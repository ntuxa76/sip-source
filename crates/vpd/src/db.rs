//! MariaDB: connection, migrations and first-start seed data. All application state lives here.

use crate::auth;
use crate::config::Boot;
use chrono::{NaiveDateTime, Utc};
use sqlx::mysql::{MySqlConnectOptions, MySqlPool, MySqlPoolOptions, MySqlSslMode};
use sqlx::{ConnectOptions, Row};
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;

pub fn utc_now() -> NaiveDateTime {
    Utc::now().naive_utc()
}

pub fn iso(t: NaiveDateTime) -> String {
    t.and_utc().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

pub async fn connect(boot: &Boot) -> Result<MySqlPool, String> {
    let options = match &boot.db_url {
        Some(url) => MySqlConnectOptions::from_str(url).map_err(|e| format!("VP_DB_URL is invalid: {e}"))?,
        None => {
            let ssl = match boot.db_ssl_mode.as_str() {
                "disabled" => MySqlSslMode::Disabled,
                "required" => MySqlSslMode::Required,
                "verify_ca" => MySqlSslMode::VerifyCa,
                "verify_identity" => MySqlSslMode::VerifyIdentity,
                _ => MySqlSslMode::Preferred,
            };
            MySqlConnectOptions::new()
                .host(&boot.db_host)
                .port(boot.db_port)
                .username(&boot.db_user)
                .password(&boot.db_password)
                .database(&boot.db_name)
                .ssl_mode(ssl)
                .charset("utf8mb4")
        }
    }
    .disable_statement_logging();

    let mut attempt = 0;
    loop {
        attempt += 1;
        let result = MySqlPoolOptions::new()
            .max_connections(boot.db_max_connections)
            .acquire_timeout(Duration::from_secs(10))
            .connect_with(options.clone())
            .await;
        match result {
            Ok(pool) => return Ok(pool),
            Err(e) if attempt < boot.db_connect_retries => {
                tracing::info!("waiting for MariaDB ({attempt}/{}): {e}", boot.db_connect_retries);
                tokio::time::sleep(Duration::from_secs(boot.db_connect_retry_seconds)).await;
            }
            Err(e) => return Err(format!("could not connect to MariaDB: {e}")),
        }
    }
}

const MIGRATIONS: &[(i32, &[&str])] = &[(
    1,
    &[
        "CREATE TABLE roles (
            id BIGINT AUTO_INCREMENT PRIMARY KEY,
            name VARCHAR(64) NOT NULL UNIQUE,
            description VARCHAR(255) NOT NULL DEFAULT '',
            permissions LONGTEXT NOT NULL,
            locked TINYINT NOT NULL DEFAULT 0,
            created_at DATETIME(6) NOT NULL,
            updated_at DATETIME(6) NOT NULL
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4",
        "CREATE TABLE users (
            id BIGINT AUTO_INCREMENT PRIMARY KEY,
            username VARCHAR(96) NOT NULL UNIQUE,
            display_name VARCHAR(128) NOT NULL DEFAULT '',
            email VARCHAR(190) NOT NULL DEFAULT '',
            password_hash VARCHAR(255) NOT NULL,
            role_id BIGINT NOT NULL,
            status VARCHAR(16) NOT NULL DEFAULT 'active',
            grants LONGTEXT NOT NULL,
            denies LONGTEXT NOT NULL,
            prefs LONGTEXT NOT NULL,
            failed_attempts INT NOT NULL DEFAULT 0,
            locked_until DATETIME(6) NULL,
            last_login_at DATETIME(6) NULL,
            created_at DATETIME(6) NOT NULL,
            updated_at DATETIME(6) NOT NULL,
            CONSTRAINT fk_users_role FOREIGN KEY (role_id) REFERENCES roles(id)
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4",
        "CREATE TABLE sessions (
            id CHAR(64) PRIMARY KEY,
            user_id BIGINT NOT NULL,
            created_at DATETIME(6) NOT NULL,
            last_seen_at DATETIME(6) NOT NULL,
            expires_at DATETIME(6) NOT NULL,
            ip VARCHAR(64) NOT NULL DEFAULT '',
            user_agent VARCHAR(255) NOT NULL DEFAULT '',
            CONSTRAINT fk_sessions_user FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4",
        "CREATE TABLE dashboards (
            id BIGINT AUTO_INCREMENT PRIMARY KEY,
            uid VARCHAR(64) NOT NULL UNIQUE,
            title VARCHAR(190) NOT NULL,
            description VARCHAR(512) NOT NULL DEFAULT '',
            body LONGTEXT NOT NULL,
            version INT NOT NULL DEFAULT 1,
            updated_by VARCHAR(96) NOT NULL DEFAULT '',
            created_at DATETIME(6) NOT NULL,
            updated_at DATETIME(6) NOT NULL
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4",
        "CREATE TABLE dashboard_versions (
            id BIGINT AUTO_INCREMENT PRIMARY KEY,
            dashboard_id BIGINT NOT NULL,
            version INT NOT NULL,
            body LONGTEXT NOT NULL,
            message VARCHAR(255) NOT NULL DEFAULT '',
            created_by VARCHAR(96) NOT NULL DEFAULT '',
            created_at DATETIME(6) NOT NULL,
            UNIQUE KEY uq_dashboard_version (dashboard_id, version),
            CONSTRAINT fk_versions_dashboard FOREIGN KEY (dashboard_id) REFERENCES dashboards(id) ON DELETE CASCADE
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4",
        "CREATE TABLE storages (
            id BIGINT AUTO_INCREMENT PRIMARY KEY,
            name VARCHAR(96) NOT NULL UNIQUE,
            type VARCHAR(32) NOT NULL,
            config LONGTEXT NOT NULL,
            secret LONGTEXT NOT NULL,
            active TINYINT NOT NULL DEFAULT 0,
            built_in TINYINT NOT NULL DEFAULT 0,
            last_test_at DATETIME(6) NULL,
            last_test_ok TINYINT NULL,
            last_test_message VARCHAR(512) NOT NULL DEFAULT '',
            created_at DATETIME(6) NOT NULL,
            updated_at DATETIME(6) NOT NULL
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4",
        "CREATE TABLE settings (
            skey VARCHAR(96) PRIMARY KEY,
            value LONGTEXT NOT NULL,
            updated_by VARCHAR(96) NOT NULL DEFAULT '',
            updated_at DATETIME(6) NOT NULL
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4",
        "CREATE TABLE saved_searches (
            id BIGINT AUTO_INCREMENT PRIMARY KEY,
            owner_id BIGINT NOT NULL,
            name VARCHAR(128) NOT NULL,
            q TEXT NOT NULL,
            shared TINYINT NOT NULL DEFAULT 0,
            created_at DATETIME(6) NOT NULL,
            CONSTRAINT fk_saved_owner FOREIGN KEY (owner_id) REFERENCES users(id) ON DELETE CASCADE
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4",
        "CREATE TABLE audit_events (
            id BIGINT AUTO_INCREMENT PRIMARY KEY,
            ts DATETIME(6) NOT NULL,
            actor VARCHAR(96) NOT NULL,
            src_ip VARCHAR(64) NOT NULL DEFAULT '',
            action VARCHAR(64) NOT NULL,
            object VARCHAR(190) NOT NULL DEFAULT '',
            outcome VARCHAR(16) NOT NULL,
            severity VARCHAR(16) NOT NULL DEFAULT 'info',
            details TEXT NOT NULL,
            KEY idx_audit_ts (ts)
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4",
        "CREATE TABLE alert_acks (
            alert_id VARCHAR(64) PRIMARY KEY,
            acked_by VARCHAR(96) NOT NULL,
            comment TEXT NOT NULL,
            acked_at DATETIME(6) NOT NULL
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4",
    ],
), (
    2,
    &[
        "DELETE FROM storages WHERE type = 'demo'",
        "DROP TABLE alert_acks",
        "CREATE TABLE agents (
            capture_id INT UNSIGNED PRIMARY KEY,
            name VARCHAR(128) NOT NULL,
            source_ip VARCHAR(64) NOT NULL DEFAULT '',
            last_seen DATETIME(6) NULL
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4",
    ],
), (
    3,
    &[
        "CREATE TABLE sources (
            ip VARCHAR(64) PRIMARY KEY,
            alias VARCHAR(128) NOT NULL DEFAULT '',
            capture_id INT UNSIGNED NOT NULL DEFAULT 0,
            last_seen DATETIME(6) NULL
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4",
        "INSERT IGNORE INTO sources (ip, alias, capture_id, last_seen)
            SELECT source_ip, IF(name LIKE 'capture-%', '', name), capture_id, last_seen FROM agents WHERE source_ip <> ''",
        "DROP TABLE agents",
    ],
)];

pub async fn migrate(pool: &MySqlPool) -> Result<(), sqlx::Error> {
    // One connection holds a named lock so that several starting instances can't race.
    let mut conn = pool.acquire().await?;
    sqlx::query("SELECT GET_LOCK('vp_migrate', 120)").execute(&mut *conn).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS schema_migrations (version INT PRIMARY KEY, applied_at DATETIME(6) NOT NULL) ENGINE=InnoDB")
        .execute(&mut *conn)
        .await?;
    for (version, statements) in MIGRATIONS {
        let done: Option<i32> = sqlx::query_scalar("SELECT version FROM schema_migrations WHERE version = ?").bind(version).fetch_optional(&mut *conn).await?;
        if done.is_some() {
            continue;
        }
        for statement in *statements {
            sqlx::query(statement).execute(&mut *conn).await?;
        }
        sqlx::query("INSERT INTO schema_migrations (version, applied_at) VALUES (?, ?)").bind(version).bind(utc_now()).execute(&mut *conn).await?;
        tracing::info!("applied database migration {version}");
    }
    sqlx::query("SELECT RELEASE_LOCK('vp_migrate')").execute(&mut *conn).await?;
    Ok(())
}

const BUILT_IN_DASHBOARDS: &[&str] = &[
    include_str!("../data/dashboards/overview.json"),
    include_str!("../data/dashboards/calls.json"),
    include_str!("../data/dashboards/registrations.json"),
    include_str!("../data/dashboards/media.json"),
    include_str!("../data/dashboards/trunks.json"),
    include_str!("../data/dashboards/nodes.json"),
    include_str!("../data/dashboards/security.json"),
    include_str!("../data/dashboards/capture.json"),
];

pub fn built_in_dashboards() -> Vec<serde_json::Value> {
    BUILT_IN_DASHBOARDS.iter().map(|s| serde_json::from_str(s).expect("built-in dashboard JSON is valid")).collect()
}

async fn marker(pool: &MySqlPool, key: &str) -> Result<bool, sqlx::Error> {
    let found: Option<String> = sqlx::query_scalar("SELECT skey FROM settings WHERE skey = ?").bind(key).fetch_optional(pool).await?;
    Ok(found.is_some())
}

async fn set_marker(pool: &MySqlPool, key: &str) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO settings (skey, value, updated_by, updated_at) VALUES (?, '1', 'system', ?)").bind(key).bind(utc_now()).execute(pool).await?;
    Ok(())
}

/// Seeds roles, dashboards and the Local file storage once. Later edits and deletes are respected.
pub async fn seed(pool: &MySqlPool) -> Result<(), sqlx::Error> {
    if !marker(pool, "_seeded_roles").await? {
        for (name, description, perms, locked) in auth::built_in_roles() {
            let body = serde_json::to_string(&perms).unwrap_or_else(|_| "[]".into());
            sqlx::query("INSERT INTO roles (name, description, permissions, locked, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)")
                .bind(name).bind(description).bind(body).bind(locked as i8).bind(utc_now()).bind(utc_now())
                .execute(pool).await?;
        }
        set_marker(pool, "_seeded_roles").await?;
    }
    if !marker(pool, "_seeded_dashboards").await? {
        for d in built_in_dashboards() {
            insert_dashboard(pool, &d, "system").await?;
        }
        set_marker(pool, "_seeded_dashboards").await?;
    }
    // Captured data has somewhere to go from the first start. It is selected unless another
    // storage already is.
    if !marker(pool, "_seeded_local_storage").await? {
        let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM storages WHERE active = 1").fetch_one(pool).await?;
        sqlx::query("INSERT INTO storages (name, type, config, secret, active, built_in, created_at, updated_at) VALUES ('Local file', 'local', ?, '', ?, 1, ?, ?)")
            .bind(serde_json::json!({"database": crate::storage::LOCAL_DATABASE}).to_string()).bind((active == 0) as i8).bind(utc_now()).bind(utc_now())
            .execute(pool).await?;
        set_marker(pool, "_seeded_local_storage").await?;
    }
    // Dashboards nobody edited pick up the panels that work on captured data.
    if !marker(pool, "_dashboards_v4").await? {
        for d in built_in_dashboards() {
            sqlx::query("UPDATE dashboards SET title = ?, description = ?, body = ?, updated_at = ? WHERE uid = ? AND version = 1 AND updated_by = 'system'")
                .bind(d["title"].as_str().unwrap_or("")).bind(d["description"].as_str().unwrap_or("")).bind(d.to_string()).bind(utc_now()).bind(d["uid"].as_str().unwrap_or(""))
                .execute(pool).await?;
        }
        set_marker(pool, "_dashboards_v4").await?;
    }
    Ok(())
}

pub async fn insert_dashboard(pool: &MySqlPool, d: &serde_json::Value, by: &str) -> Result<i64, sqlx::Error> {
    let body = d.to_string();
    let res = sqlx::query("INSERT INTO dashboards (uid, title, description, body, version, updated_by, created_at, updated_at) VALUES (?, ?, ?, ?, 1, ?, ?, ?)")
        .bind(d["uid"].as_str().unwrap_or(""))
        .bind(d["title"].as_str().unwrap_or(""))
        .bind(d["description"].as_str().unwrap_or(""))
        .bind(&body)
        .bind(by)
        .bind(utc_now())
        .bind(utc_now())
        .execute(pool)
        .await?;
    let id = res.last_insert_id() as i64;
    sqlx::query("INSERT INTO dashboard_versions (dashboard_id, version, body, message, created_by, created_at) VALUES (?, 1, ?, 'Created', ?, ?)")
        .bind(id).bind(&body).bind(by).bind(utc_now()).execute(pool).await?;
    Ok(id)
}

pub async fn load_settings(pool: &MySqlPool) -> Result<HashMap<String, String>, sqlx::Error> {
    let rows = sqlx::query("SELECT skey, value FROM settings WHERE skey NOT LIKE '\\_%'").fetch_all(pool).await?;
    Ok(rows.iter().map(|r| (r.get::<String, _>("skey"), r.get::<String, _>("value"))).collect())
}

/// Records an audit event without blocking the request that caused it.
pub fn audit(pool: &MySqlPool, actor: &str, ip: &str, action: &str, object: &str, outcome: &str, severity: &str, details: &str) {
    let (pool, actor, ip, action, object, outcome, severity, details) =
        (pool.clone(), actor.to_string(), ip.to_string(), action.to_string(), object.to_string(), outcome.to_string(), severity.to_string(), details.to_string());
    tokio::spawn(async move {
        let result = sqlx::query("INSERT INTO audit_events (ts, actor, src_ip, action, object, outcome, severity, details) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(utc_now()).bind(actor).bind(ip).bind(action).bind(object).bind(outcome).bind(severity).bind(details)
            .execute(&pool).await;
        if let Err(e) = result {
            tracing::warn!("could not write audit event: {e}");
        }
    });
}
