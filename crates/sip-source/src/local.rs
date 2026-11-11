//! The "Local file" storage: a ClickHouse engine bundled in the image, run as a child process that
//! keeps its files under the data directory. Every query works the same as on a ClickHouse server.

use crate::ch::Ch;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::process::{Child, Command};

pub struct Engine {
    binary: String,
    dir: PathBuf,
    port: u16,
    password: String,
    /// For user-written SQL: may only read the capture database.
    reader_password: String,
    child: Mutex<Option<Child>>,
    /// Whether the active storage wants the engine running. A crash restarts it only while true.
    wanted: AtomicBool,
    /// Only one caller starts the engine at a time; the others wait and reuse it.
    starting: tokio::sync::Mutex<()>,
}

impl Engine {
    pub fn new(binary: String, data_dir: &str, port: u16) -> Engine {
        // A fresh password every start: nothing outside this process ever needs it.
        let secret = || {
            let mut bytes = [0u8; 24];
            rand::rngs::OsRng.fill_bytes(&mut bytes);
            hex::encode(bytes)
        };
        Engine { binary, dir: PathBuf::from(data_dir).join("engine"), port, password: secret(), reader_password: secret(), child: Mutex::new(None), wanted: AtomicBool::new(false), starting: tokio::sync::Mutex::new(()) }
    }

    pub fn client(&self, database: &str) -> Result<Ch, String> {
        Ch::new(format!("http://127.0.0.1:{}", self.port), database.to_string(), "default".into(), self.password.clone())
            .map(|c| c.with_reader("reader".into(), self.reader_password.clone()))
    }

    fn write_config(&self) -> Result<PathBuf, String> {
        let d = &self.dir;
        for sub in ["", "tmp", "user_files", "format_schemas"] {
            std::fs::create_dir_all(d.join(sub)).map_err(|e| format!("can't create {}: {e}", d.join(sub).display()))?;
        }
        let path = |p: &str| format!("{}/{p}", d.display());
        let config = format!(
            r#"<clickhouse>
    <logger><level>error</level><console>1</console></logger>
    <listen_host>127.0.0.1</listen_host>
    <http_port>{port}</http_port>
    <path>{data}</path>
    <tmp_path>{tmp}</tmp_path>
    <user_files_path>{files}</user_files_path>
    <format_schema_path>{schemas}</format_schema_path>
    <users_config>users.xml</users_config>
    <mark_cache_size>268435456</mark_cache_size>
    <uncompressed_cache_size>0</uncompressed_cache_size>
    <max_server_memory_usage_to_ram_ratio>0.5</max_server_memory_usage_to_ram_ratio>
    <max_concurrent_queries>100</max_concurrent_queries>
    <mlock_executable>false</mlock_executable>
    <background_schedule_pool_size>16</background_schedule_pool_size>
    <background_common_pool_size>4</background_common_pool_size>
    <background_move_pool_size>2</background_move_pool_size>
    <background_fetches_pool_size>2</background_fetches_pool_size>
    <background_buffer_flush_schedule_pool_size>2</background_buffer_flush_schedule_pool_size>
    <background_message_broker_schedule_pool_size>2</background_message_broker_schedule_pool_size>
    <background_distributed_schedule_pool_size>2</background_distributed_schedule_pool_size>
    <send_crash_reports><enabled>false</enabled></send_crash_reports>
</clickhouse>
"#,
            port = self.port,
            data = path(""),
            tmp = path("tmp/"),
            files = path("user_files/"),
            schemas = path("format_schemas/"),
        );
        let users = format!(
            r#"<clickhouse>
    <profiles><default/></profiles>
    <quotas><default/></quotas>
    <users>
        <default>
            <password_sha256_hex>{}</password_sha256_hex>
            <networks><ip>127.0.0.1</ip><ip>::1</ip></networks>
            <profile>default</profile>
            <quota>default</quota>
        </default>
        <reader>
            <password_sha256_hex>{}</password_sha256_hex>
            <networks><ip>127.0.0.1</ip><ip>::1</ip></networks>
            <profile>default</profile>
            <quota>default</quota>
            <grants><query>GRANT SELECT ON {}.*</query></grants>
        </reader>
    </users>
</clickhouse>
"#,
            hex::encode(Sha256::digest(self.password.as_bytes())),
            hex::encode(Sha256::digest(self.reader_password.as_bytes())),
            crate::storage::LOCAL_DATABASE,
        );
        let file = d.join("config.xml");
        std::fs::write(&file, config).map_err(|e| format!("can't write {}: {e}", file.display()))?;
        std::fs::write(d.join("users.xml"), users).map_err(|e| format!("can't write users.xml: {e}"))?;
        Ok(file)
    }

    fn running(&self) -> bool {
        let mut child = self.child.lock().unwrap();
        match child.as_mut().map(|c| c.try_wait()) {
            Some(Ok(None)) => true,
            Some(_) => {
                *child = None;
                false
            }
            None => false,
        }
    }

    fn spawn(&self) -> Result<(), String> {
        let config = self.write_config()?;
        let child = Command::new(&self.binary)
            .arg("server")
            .arg(format!("--config-file={}", config.display()))
            .current_dir(&self.dir)
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("can't start the local storage engine ({}): {e}", self.binary))?;
        *self.child.lock().unwrap() = Some(child);
        Ok(())
    }

    /// Starts the engine if needed and waits until it answers queries.
    pub async fn start(&self) -> Result<(), String> {
        let _one = self.starting.lock().await;
        self.wanted.store(true, Ordering::SeqCst);
        if !self.running() {
            tracing::info!("starting the local storage engine in {}", self.dir.display());
            self.spawn()?;
        }
        let probe = self.client("system")?;
        for _ in 0..240 {
            if probe.version().await.is_ok() {
                return Ok(());
            }
            if !self.running() {
                return Err("the local storage engine stopped while starting; its reason is in the log just above".into());
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        Err("the local storage engine didn't answer within 60 seconds".into())
    }

    /// Stops the engine cleanly, for example after switching to a ClickHouse server or on shutdown.
    pub async fn stop(&self) {
        self.wanted.store(false, Ordering::SeqCst);
        let child = self.child.lock().unwrap().take();
        let Some(mut child) = child else { return };
        tracing::info!("stopping the local storage engine");
        if let Some(pid) = child.id() {
            // SAFETY: plain signal to our own child process.
            unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        }
        if tokio::time::timeout(Duration::from_secs(20), child.wait()).await.is_err() {
            let _ = child.kill().await;
        }
    }

    /// Restarts the engine if it dies while it is the active storage.
    pub async fn watch(&self) {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            if self.wanted.load(Ordering::SeqCst) && !self.running() {
                tracing::warn!("the local storage engine exited, restarting it");
                if let Err(e) = self.start().await {
                    tracing::error!("{e}");
                }
            }
        }
    }
}
