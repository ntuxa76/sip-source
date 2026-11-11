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
        Some(url) => MySqlConnectOptions::from_str(url).map_err(|e| format!("SS_DB_URL is invalid: {e}"))?,
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

/// The whole database schema, MariaDB and ClickHouse, in one file at the repository root.
const SCHEMA: &str = include_str!("../../../schema.sql");

/// The statements of one part of `schema.sql`, the lines after `-- @<name>` up to the next part.
pub fn schema_part(name: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut inside = false;
    for line in SCHEMA.lines() {
        if let Some(header) = line.strip_prefix("-- @") {
            inside = header.trim() == name;
            continue;
        }
        if inside {
            lines.push(line);
        }
    }
    statements(&lines.join("\n"))
}

/// The schema version of the `@mariadb` part: the highest `@mariadb-upgrade` number.
fn latest_version() -> i32 {
    SCHEMA.lines().filter_map(|l| l.strip_prefix("-- @mariadb-upgrade ")).filter_map(|n| n.trim().parse().ok()).max().unwrap_or(1)
}

/// Splits a schema file into statements: `--` comment lines are dropped, and a statement ends with
/// a semicolon at the end of a line.
pub fn statements(file: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for line in file.lines() {
        if line.trim_start().starts_with("--") {
            continue;
        }
        current.push_str(line);
        current.push('\n');
        if line.trim_end().ends_with(';') {
            let statement = current.trim().trim_end_matches(';').trim().to_string();
            if !statement.is_empty() {
                out.push(statement);
            }
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

pub async fn migrate(pool: &MySqlPool) -> Result<(), sqlx::Error> {
    // One connection holds a named lock so that several starting instances can't race.
    let mut conn = pool.acquire().await?;
    sqlx::query("SELECT GET_LOCK('ss_migrate', 120)").execute(&mut *conn).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS schema_migrations (version INT PRIMARY KEY, applied_at DATETIME(6) NOT NULL) ENGINE=InnoDB")
        .execute(&mut *conn)
        .await?;
    let applied: Vec<i32> = sqlx::query_scalar("SELECT version FROM schema_migrations").fetch_all(&mut *conn).await?;
    let latest = latest_version();
    let record = |version: i32| sqlx::query("INSERT INTO schema_migrations (version, applied_at) VALUES (?, ?)").bind(version).bind(utc_now());
    if applied.is_empty() {
        // An empty database gets the current schema in one go.
        for statement in schema_part("mariadb") {
            sqlx::query(&statement).execute(&mut *conn).await?;
        }
        for version in 1..=latest {
            record(version).execute(&mut *conn).await?;
        }
        tracing::info!("created the database schema, version {latest}");
    } else {
        // A database from an older version gets each upgrade it is missing, in order.
        for version in 2..=latest {
            if applied.contains(&version) {
                continue;
            }
            for statement in schema_part(&format!("mariadb-upgrade {version}")) {
                sqlx::query(&statement).execute(&mut *conn).await?;
            }
            record(version).execute(&mut *conn).await?;
            tracing::info!("upgraded the database schema to version {version}");
        }
    }
    sqlx::query("SELECT RELEASE_LOCK('ss_migrate')").execute(&mut *conn).await?;
    Ok(())
}

const BUILT_IN_DASHBOARDS: &[&str] = &[
    include_str!("../dashboards/overview.json"),
    include_str!("../dashboards/calls.json"),
    include_str!("../dashboards/registrations.json"),
    include_str!("../dashboards/media.json"),
    include_str!("../dashboards/trunks.json"),
    include_str!("../dashboards/nodes.json"),
    include_str!("../dashboards/security.json"),
    include_str!("../dashboards/capture.json"),
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
    // Earlier versions named the roles differently. Rename the ones nobody changed, and add the
    // built-in roles that are new, once.
    if !marker(pool, "_roles_v2").await? {
        let roles = auth::built_in_roles();
        for (old, new, old_description) in auth::ROLE_RENAMES {
            let Some((_, description, _, _)) = roles.iter().find(|r| r.0 == *new) else { continue };
            let taken: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM roles WHERE name = ?").bind(new).fetch_one(pool).await?;
            if old != new && taken == 0 {
                sqlx::query("UPDATE roles SET name = ?, updated_at = ? WHERE name = ?").bind(new).bind(utc_now()).bind(old).execute(pool).await?;
            }
            sqlx::query("UPDATE roles SET description = ?, updated_at = ? WHERE name = ? AND description = ?").bind(description).bind(utc_now()).bind(new).bind(old_description).execute(pool).await?;
        }
        // Only roles that are new in this version; built-in roles someone deleted stay deleted.
        for (name, description, perms, locked) in roles.into_iter().filter(|r| !auth::ROLE_RENAMES.iter().any(|x| x.1 == r.0)) {
            sqlx::query("INSERT IGNORE INTO roles (name, description, permissions, locked, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)")
                .bind(name).bind(description).bind(serde_json::to_string(&perms).unwrap_or_else(|_| "[]".into())).bind(locked as i8).bind(utc_now()).bind(utc_now())
                .execute(pool).await?;
        }
        set_marker(pool, "_roles_v2").await?;
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
    // v5 renamed the storage panels of Capture health, which no longer say ClickHouse.
    if !marker(pool, "_dashboards_v5").await? {
        for d in built_in_dashboards() {
            sqlx::query("UPDATE dashboards SET title = ?, description = ?, body = ?, updated_at = ? WHERE uid = ? AND version = 1 AND updated_by = 'system'")
                .bind(d["title"].as_str().unwrap_or("")).bind(d["description"].as_str().unwrap_or("")).bind(d.to_string()).bind(utc_now()).bind(d["uid"].as_str().unwrap_or(""))
                .execute(pool).await?;
        }
        set_marker(pool, "_dashboards_v5").await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every part of schema.sql is present and splits into whole statements.
    #[test]
    fn schema_parts() {
        let current = schema_part("mariadb");
        assert!(current.iter().any(|s| s.starts_with("CREATE TABLE users")));
        assert!(current.iter().all(|s| !s.contains(';') || s.contains("CHARSET=utf8mb4") || s.contains('\'')));
        assert!(latest_version() >= 4);
        for version in 2..=latest_version() {
            assert!(!schema_part(&format!("mariadb-upgrade {version}")).is_empty(), "upgrade {version} is empty");
        }
        let ch = schema_part("clickhouse");
        assert!(ch.iter().any(|s| s.starts_with("CREATE TABLE IF NOT EXISTS calls")));
        assert!(ch.iter().any(|s| s.starts_with("CREATE OR REPLACE VIEW v_calls")));
        assert!(ch.iter().all(|s| !s.ends_with(';')));
    }
}
