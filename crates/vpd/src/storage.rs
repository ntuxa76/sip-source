//! Capture-data storages. Connections are chosen in the UI and kept in MariaDB, and one is active.

use crate::api::{App, problem};
use crate::auth::{User, db_error, is_duplicate};
use crate::db::{self, iso, utc_now};
use crate::ch::{self, Ch};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::sync::Arc;

type S = State<Arc<App>>;

fn cipher(secret: &str) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new_from_slice(&Sha256::digest(secret.as_bytes())).expect("key is 32 bytes")
}

/// Encrypts with VP_SECRET_KEY when it is set. Without it the value is stored as given.
fn seal(a: &App, plain: &str) -> String {
    match &a.boot.secret_key {
        None => format!("plain:{plain}"),
        Some(key) => {
            let mut nonce = [0u8; 12];
            rand::rngs::OsRng.fill_bytes(&mut nonce);
            let ct = cipher(key).encrypt(Nonce::from_slice(&nonce), plain.as_bytes()).expect("encryption does not fail");
            format!("enc:{}:{}", B64.encode(nonce), B64.encode(ct))
        }
    }
}

fn open(a: &App, stored: &str) -> Result<String, String> {
    if let Some(p) = stored.strip_prefix("plain:") {
        return Ok(p.to_string());
    }
    let Some(rest) = stored.strip_prefix("enc:") else { return Ok(String::new()) };
    let key = a.boot.secret_key.as_ref().ok_or("This password was encrypted, but VP_SECRET_KEY is not set.")?;
    let (nonce, ct) = rest.split_once(':').ok_or("Stored password is damaged.")?;
    let nonce = B64.decode(nonce).map_err(|e| e.to_string())?;
    let ct = B64.decode(ct).map_err(|e| e.to_string())?;
    let plain = cipher(key).decrypt(Nonce::from_slice(&nonce), ct.as_slice()).map_err(|_| "Could not decrypt the stored password. Has VP_SECRET_KEY changed?".to_string())?;
    String::from_utf8(plain).map_err(|e| e.to_string())
}

fn clickhouse_from(a: &App, config: &Value, secret: &str) -> Result<Ch, String> {
    let scheme = if config["tls"].as_bool().unwrap_or(false) { "https" } else { "http" };
    let host = config["host"].as_str().unwrap_or("").trim();
    if host.is_empty() {
        return Err("Host is required.".into());
    }
    Ch::new(
        format!("{scheme}://{host}:{}", config["port"].as_u64().unwrap_or(8123)),
        config["database"].as_str().unwrap_or("voip_panel").to_string(),
        config["username"].as_str().unwrap_or("default").to_string(),
        open(a, secret)?,
    )
}

/// Reloads the active storage into memory. Called at startup and after any storage change.
pub async fn refresh(a: &App) -> Result<(), sqlx::Error> {
    let row = sqlx::query("SELECT type, config, secret FROM storages WHERE active = 1 LIMIT 1").fetch_optional(&a.db).await?;
    let active = row.and_then(|r| {
        let config: Value = serde_json::from_str(&r.get::<String, _>("config")).unwrap_or_else(|_| json!({}));
        match clickhouse_from(a, &config, &r.get::<String, _>("secret")) {
            Ok(c) => Some(c),
            Err(e) => {
                tracing::error!("the selected storage is unusable: {e}");
                None
            }
        }
    });
    *a.storage.write().unwrap() = active;
    Ok(())
}

fn retention(a: &App) -> ch::Retention {
    let s = &a.settings;
    ch::Retention {
        messages: s.int("retention_messages_days"),
        registrations: s.int("retention_registrations_days"),
        qos: s.int("retention_qos_days"),
        logs: s.int("retention_logs_days"),
        calls: s.int("retention_calls_days"),
        metrics: s.int("retention_metrics_days"),
    }
}

/// Brings the selected ClickHouse up to the current schema, waiting for it if it isn't reachable.
pub async fn prepare_active(a: Arc<App>) {
    loop {
        let current = a.storage.read().unwrap().clone();
        let Some(c) = current else { return };
        match ch::init_schema(&c, &retention(&a)).await {
            Ok(()) => return,
            Err(e) => {
                tracing::warn!("ClickHouse is not ready: {e}");
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
        }
    }
}

fn storage_json(r: &sqlx::mysql::MySqlRow) -> Value {
    let tested: Option<chrono::NaiveDateTime> = r.get("last_test_at");
    let config: Value = serde_json::from_str(&r.get::<String, _>("config")).unwrap_or_else(|_| json!({}));
    let secret: String = r.get("secret");
    json!({
        "id": r.get::<i64, _>("id"), "name": r.get::<String, _>("name"), "type": r.get::<String, _>("type"), "config": config,
        "active": r.get::<i8, _>("active") != 0, "built_in": r.get::<i8, _>("built_in") != 0,
        "has_password": !secret.is_empty() && secret != "plain:", "encrypted": secret.starts_with("enc:"),
        "last_test_at": tested.map(iso), "last_test_ok": r.get::<Option<i8>, _>("last_test_ok").map(|v| v != 0), "last_test_message": r.get::<String, _>("last_test_message"),
    })
}

pub async fn list(State(a): S, user: User) -> Response {
    if let Err(r) = user.need("storage_manage") {
        return r;
    }
    match sqlx::query("SELECT * FROM storages ORDER BY built_in DESC, name").fetch_all(&a.db).await {
        Ok(rows) => Json(json!({"storages": rows.iter().map(storage_json).collect::<Vec<_>>(), "encryption_key_set": a.boot.secret_key.is_some()})).into_response(),
        Err(e) => db_error(e),
    }
}

/// Which storage is in use, so every page can tell people what they are looking at.
pub async fn active(State(a): S, _user: User) -> Response {
    match sqlx::query("SELECT name, type FROM storages WHERE active = 1 LIMIT 1").fetch_optional(&a.db).await {
        Ok(Some(r)) => Json(json!({"name": r.get::<String, _>("name"), "type": r.get::<String, _>("type")})).into_response(),
        Ok(None) => Json(json!({"name": "", "type": "none"})).into_response(),
        Err(e) => db_error(e),
    }
}

#[derive(Deserialize)]
pub struct Body {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    config: Value,
    #[serde(default)]
    password: Option<String>,
}

fn check_type(kind: &str) -> Result<(), Response> {
    if kind == "clickhouse" {
        Ok(())
    } else {
        Err(problem(StatusCode::BAD_REQUEST, "Unsupported storage type", "Only ClickHouse storages can be added."))
    }
}

pub async fn create(State(a): S, user: User, Json(b): Json<Body>) -> Response {
    if let Err(r) = user.need("storage_manage").and_then(|_| check_type(&b.kind)) {
        return r;
    }
    let secret = seal(&a, b.password.as_deref().unwrap_or(""));
    let now = utc_now();
    match sqlx::query("INSERT INTO storages (name, type, config, secret, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(b.name.trim()).bind(&b.kind).bind(b.config.to_string()).bind(secret).bind(now).bind(now).execute(&a.db).await {
        Ok(r) => {
            db::audit(&a.db, &user.username, &user.ip, "storage.create", b.name.trim(), "success", "notice", "");
            Json(json!({"id": r.last_insert_id()})).into_response()
        }
        Err(e) if is_duplicate(&e) => problem(StatusCode::CONFLICT, "Name in use", "A storage with this name already exists."),
        Err(e) => db_error(e),
    }
}

pub async fn update(State(a): S, user: User, Path(id): Path<i64>, Json(b): Json<Body>) -> Response {
    if let Err(r) = user.need("storage_manage").and_then(|_| check_type(&b.kind)) {
        return r;
    }
    // Leaving the password blank keeps the stored one.
    let result = match b.password.as_deref().filter(|p| !p.is_empty()) {
        Some(p) => sqlx::query("UPDATE storages SET name = ?, config = ?, secret = ?, updated_at = ? WHERE id = ? AND built_in = 0").bind(b.name.trim()).bind(b.config.to_string()).bind(seal(&a, p)).bind(utc_now()).bind(id).execute(&a.db).await,
        None => sqlx::query("UPDATE storages SET name = ?, config = ?, updated_at = ? WHERE id = ? AND built_in = 0").bind(b.name.trim()).bind(b.config.to_string()).bind(utc_now()).bind(id).execute(&a.db).await,
    };
    match result {
        Ok(_) => {
            if let Err(e) = refresh(&a).await {
                return db_error(e);
            }
            tokio::spawn(prepare_active(a.clone()));
            db::audit(&a.db, &user.username, &user.ip, "storage.update", b.name.trim(), "success", "notice", "");
            Json(json!({"ok": true})).into_response()
        }
        Err(e) if is_duplicate(&e) => problem(StatusCode::CONFLICT, "Name in use", "A storage with this name already exists."),
        Err(e) => db_error(e),
    }
}

pub async fn remove(State(a): S, user: User, Path(id): Path<i64>) -> Response {
    if let Err(r) = user.need("storage_manage") {
        return r;
    }
    let row = match sqlx::query("SELECT name, active FROM storages WHERE id = ?").bind(id).fetch_optional(&a.db).await {
        Ok(Some(r)) => r,
        Ok(None) => return problem(StatusCode::NOT_FOUND, "Not found", "No such storage."),
        Err(e) => return db_error(e),
    };
    if row.get::<i8, _>("active") != 0 {
        return problem(StatusCode::CONFLICT, "In use", "Select another storage first.");
    }
    match sqlx::query("DELETE FROM storages WHERE id = ?").bind(id).execute(&a.db).await {
        Ok(_) => {
            db::audit(&a.db, &user.username, &user.ip, "storage.delete", &row.get::<String, _>("name"), "success", "notice", "");
            Json(json!({"ok": true})).into_response()
        }
        Err(e) => db_error(e),
    }
}

pub async fn test(State(a): S, user: User, Path(id): Path<i64>) -> Response {
    if let Err(r) = user.need("storage_manage") {
        return r;
    }
    let row = match sqlx::query("SELECT type, config, secret FROM storages WHERE id = ?").bind(id).fetch_optional(&a.db).await {
        Ok(Some(r)) => r,
        Ok(None) => return problem(StatusCode::NOT_FOUND, "Not found", "No such storage."),
        Err(e) => return db_error(e),
    };
    let config: Value = serde_json::from_str(&row.get::<String, _>("config")).unwrap_or_else(|_| json!({}));
    let result = match clickhouse_from(&a, &config, &row.get::<String, _>("secret")) {
        Ok(c) => c.version().await.map(|v| format!("Connected. ClickHouse {v}")),
        Err(e) => Err(e),
    };
    let (ok, message) = match &result {
        Ok(m) => (true, m.clone()),
        Err(e) => (false, e.clone()),
    };
    let short: String = message.chars().take(500).collect();
    let _ = sqlx::query("UPDATE storages SET last_test_at = ?, last_test_ok = ?, last_test_message = ? WHERE id = ?").bind(utc_now()).bind(ok as i8).bind(&short).bind(id).execute(&a.db).await;
    Json(json!({"ok": ok, "message": short})).into_response()
}

pub async fn activate(State(a): S, user: User, Path(id): Path<i64>) -> Response {
    if let Err(r) = user.need("storage_manage") {
        return r;
    }
    let row = match sqlx::query("SELECT name, config, secret FROM storages WHERE id = ?").bind(id).fetch_optional(&a.db).await {
        Ok(Some(r)) => r,
        Ok(None) => return problem(StatusCode::NOT_FOUND, "Not found", "No such storage."),
        Err(e) => return db_error(e),
    };
    let name: String = row.get("name");
    let config: Value = serde_json::from_str(&row.get::<String, _>("config")).unwrap_or_else(|_| json!({}));
    // The tables must exist before capture starts writing here.
    let prepared = match clickhouse_from(&a, &config, &row.get::<String, _>("secret")) {
        Ok(c) => ch::init_schema(&c, &retention(&a)).await,
        Err(e) => Err(e),
    };
    if let Err(e) = prepared {
        return problem(StatusCode::UNPROCESSABLE_ENTITY, "Could not prepare ClickHouse", &e);
    }
    let mut tx = match a.db.begin().await {
        Ok(t) => t,
        Err(e) => return db_error(e),
    };
    if let Err(e) = sqlx::query("UPDATE storages SET active = 0").execute(&mut *tx).await {
        return db_error(e);
    }
    if let Err(e) = sqlx::query("UPDATE storages SET active = 1 WHERE id = ?").bind(id).execute(&mut *tx).await {
        return db_error(e);
    }
    if let Err(e) = tx.commit().await {
        return db_error(e);
    }
    if let Err(e) = refresh(&a).await {
        return db_error(e);
    }
    db::audit(&a.db, &user.username, &user.ip, "storage.activate", &name, "success", "notice", "");
    Json(json!({"ok": true})).into_response()
}
