//! Dashboards. Each one is a single JSON document, so pasting the JSON recreates the whole thing.

use crate::api::{App, problem};
use crate::auth::{User, db_error, is_duplicate};
use crate::db::{self, built_in_dashboards, iso, utc_now};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use std::collections::HashSet;
use std::sync::Arc;

type S = State<Arc<App>>;

pub const SCHEMA_VERSION: i64 = 1;
const PANEL_TYPES: &[&str] = &["stat", "timeseries", "top", "table", "heatmap", "histogram", "calls", "failing_aors", "text"];

fn bad(detail: impl AsRef<str>) -> Response {
    problem(StatusCode::BAD_REQUEST, "Invalid dashboard", detail.as_ref())
}

fn slug(title: &str) -> String {
    let s: String = title.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let trimmed: String = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    let mut suffix = [0u8; 3];
    rand::rngs::OsRng.fill_bytes(&mut suffix);
    format!("{}-{}", if trimmed.is_empty() { "dashboard" } else { &trimmed[..trimmed.len().min(48)] }, hex::encode(suffix))
}

fn valid_uid(uid: &str) -> bool {
    !uid.is_empty() && uid.len() <= 64 && uid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Checks a dashboard document and fills in anything optional.
fn normalize(mut d: Value) -> Result<Value, Response> {
    let Some(obj) = d.as_object_mut() else { return Err(bad("A dashboard must be a JSON object.")) };
    let title = obj.get("title").and_then(Value::as_str).unwrap_or("").trim().to_string();
    if title.is_empty() {
        return Err(bad("\"title\" is required."));
    }
    obj.insert("schemaVersion".into(), json!(SCHEMA_VERSION));
    obj.entry("description").or_insert(json!(""));
    obj.entry("tags").or_insert(json!([]));
    obj.entry("time").or_insert(json!({"from": "now-6h", "to": "now"}));
    obj.entry("refresh").or_insert(json!("30s"));
    obj.entry("sharedCrosshair").or_insert(Value::Null);
    obj.entry("variables").or_insert(json!([]));
    obj.entry("panels").or_insert(json!([]));
    let panels = obj.get_mut("panels").and_then(Value::as_array_mut).ok_or_else(|| bad("\"panels\" must be a list."))?;
    let mut seen = HashSet::new();
    for (i, p) in panels.iter_mut().enumerate() {
        let where_ = format!("Panel {}", i + 1);
        let Some(po) = p.as_object_mut() else { return Err(bad(format!("{where_} must be an object."))) };
        let kind = po.get("type").and_then(Value::as_str).unwrap_or("");
        if !PANEL_TYPES.contains(&kind) {
            return Err(bad(format!("{where_} has an unknown type \"{kind}\". Use one of: {}.", PANEL_TYPES.join(", "))));
        }
        let id = po.get("id").and_then(Value::as_i64).ok_or_else(|| bad(format!("{where_} needs a numeric \"id\".")))?;
        if !seen.insert(id) {
            return Err(bad(format!("Panel id {id} is used twice.")));
        }
        let g = po.get("gridPos").ok_or_else(|| bad(format!("{where_} needs \"gridPos\".")))?;
        for key in ["x", "y", "w", "h"] {
            if g.get(key).and_then(Value::as_i64).is_none() {
                return Err(bad(format!("{where_}: gridPos.{key} must be a number.")));
            }
        }
        po.entry("title").or_insert(json!(""));
        po.entry("query").or_insert(json!({}));
        po.entry("options").or_insert(json!({}));
    }
    Ok(d)
}

fn panel_sql_set(d: &Value) -> HashSet<String> {
    d["panels"].as_array().map(|ps| {
        ps.iter().filter(|p| p["query"]["mode"] == "sql").filter_map(|p| p["query"]["sql"].as_str().map(|s| s.trim().to_string())).collect()
    }).unwrap_or_default()
}

/// Without the SQL permission, a save may keep existing panel SQL but not add or change any.
fn check_sql_change(user: &User, old: Option<&Value>, new: &Value) -> Result<(), Response> {
    if user.can("dashboards_sql") {
        return Ok(());
    }
    let allowed = old.map(panel_sql_set).unwrap_or_default();
    if panel_sql_set(new).iter().all(|s| allowed.contains(s)) {
        Ok(())
    } else {
        Err(problem(StatusCode::FORBIDDEN, "Not allowed", "Your account can't add or change SQL in panels."))
    }
}

pub async fn list(State(a): S, user: User) -> Response {
    if let Err(r) = user.need("dashboards_view") {
        return r;
    }
    match sqlx::query("SELECT uid, title, description, body, version, updated_by, updated_at FROM dashboards ORDER BY title").fetch_all(&a.db).await {
        Ok(rows) => Json(json!(rows.iter().map(|r| {
            let body: Value = serde_json::from_str(&r.get::<String, _>("body")).unwrap_or_else(|_| json!({}));
            json!({
                "uid": r.get::<String, _>("uid"), "title": r.get::<String, _>("title"), "description": r.get::<String, _>("description"),
                "tags": body["tags"], "panels": body["panels"].as_array().map(Vec::len).unwrap_or(0), "version": r.get::<i32, _>("version"),
                "updated_by": r.get::<String, _>("updated_by"), "updated_at": iso(r.get("updated_at")),
            })
        }).collect::<Vec<_>>())).into_response(),
        Err(e) => db_error(e),
    }
}

pub async fn get(State(a): S, user: User, Path(uid): Path<String>) -> Response {
    if let Err(r) = user.need("dashboards_view") {
        return r;
    }
    match sqlx::query("SELECT body, version, updated_by, updated_at FROM dashboards WHERE uid = ?").bind(&uid).fetch_optional(&a.db).await {
        Ok(Some(r)) => {
            let body: Value = serde_json::from_str(&r.get::<String, _>("body")).unwrap_or_else(|_| json!({}));
            Json(json!({"dashboard": body, "meta": {"version": r.get::<i32, _>("version"), "updated_by": r.get::<String, _>("updated_by"), "updated_at": iso(r.get("updated_at"))}})).into_response()
        }
        Ok(None) => problem(StatusCode::NOT_FOUND, "Not found", "No such dashboard."),
        Err(e) => db_error(e),
    }
}

#[derive(Deserialize)]
pub struct CreateBody {
    dashboard: Value,
    #[serde(default)]
    overwrite: bool,
}

/// Creates from JSON. A new UID is assigned if the document has none.
pub async fn create(State(a): S, user: User, Json(b): Json<CreateBody>) -> Response {
    if let Err(r) = user.need("dashboards_edit") {
        return r;
    }
    let mut d = match normalize(b.dashboard) {
        Ok(d) => d,
        Err(r) => return r,
    };
    let uid = match d["uid"].as_str().filter(|u| !u.is_empty()) {
        Some(u) if valid_uid(u) => u.to_string(),
        Some(_) => return bad("\"uid\" may only contain letters, digits, - and _ (up to 64 characters)."),
        None => slug(d["title"].as_str().unwrap_or("")),
    };
    d["uid"] = json!(uid);

    if b.overwrite {
        let exists: Option<i64> = match sqlx::query_scalar("SELECT id FROM dashboards WHERE uid = ?").bind(&uid).fetch_optional(&a.db).await {
            Ok(v) => v,
            Err(e) => return db_error(e),
        };
        if exists.is_some() {
            return save(&a, &user, &uid, d, None, "Imported").await;
        }
    }
    if let Err(r) = check_sql_change(&user, None, &d) {
        return r;
    }
    match db::insert_dashboard(&a.db, &d, &user.username).await {
        Ok(_) => {
            db::audit(&a.db, &user.username, &user.ip, "dashboard.create", &uid, "success", "notice", "");
            Json(json!({"uid": uid})).into_response()
        }
        Err(e) if is_duplicate(&e) => problem(StatusCode::CONFLICT, "UID in use", &format!("A dashboard with uid \"{uid}\" already exists. Import it as a copy, or overwrite it.")),
        Err(e) => db_error(e),
    }
}

#[derive(Deserialize)]
pub struct UpdateBody {
    dashboard: Value,
    #[serde(default)]
    version: Option<i32>,
    #[serde(default)]
    message: String,
}

pub async fn update(State(a): S, user: User, Path(uid): Path<String>, Json(b): Json<UpdateBody>) -> Response {
    if let Err(r) = user.need("dashboards_edit") {
        return r;
    }
    let mut d = match normalize(b.dashboard) {
        Ok(d) => d,
        Err(r) => return r,
    };
    d["uid"] = json!(uid);
    save(&a, &user, &uid, d, b.version, &b.message).await
}

async fn save(a: &App, user: &User, uid: &str, d: Value, expected: Option<i32>, message: &str) -> Response {
    let row = match sqlx::query("SELECT id, body, version FROM dashboards WHERE uid = ?").bind(uid).fetch_optional(&a.db).await {
        Ok(Some(r)) => r,
        Ok(None) => return problem(StatusCode::NOT_FOUND, "Not found", "No such dashboard."),
        Err(e) => return db_error(e),
    };
    let old: Value = serde_json::from_str(&row.get::<String, _>("body")).unwrap_or_else(|_| json!({}));
    if let Err(r) = check_sql_change(user, Some(&old), &d) {
        return r;
    }
    let (id, current): (i64, i32) = (row.get("id"), row.get("version"));
    if expected.is_some_and(|v| v != current) {
        return problem(StatusCode::CONFLICT, "Changed by someone else", "Someone saved this dashboard after you opened it. Reload to get their changes.");
    }
    let next = current + 1;
    let body = d.to_string();
    let mut tx = match a.db.begin().await {
        Ok(t) => t,
        Err(e) => return db_error(e),
    };
    let write = sqlx::query("UPDATE dashboards SET title = ?, description = ?, body = ?, version = ?, updated_by = ?, updated_at = ? WHERE id = ?")
        .bind(d["title"].as_str().unwrap_or("")).bind(d["description"].as_str().unwrap_or("")).bind(&body).bind(next).bind(&user.username).bind(utc_now()).bind(id)
        .execute(&mut *tx).await;
    if let Err(e) = write {
        return db_error(e);
    }
    if let Err(e) = sqlx::query("INSERT INTO dashboard_versions (dashboard_id, version, body, message, created_by, created_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(id).bind(next).bind(&body).bind(message.chars().take(250).collect::<String>()).bind(&user.username).bind(utc_now()).execute(&mut *tx).await {
        return db_error(e);
    }
    let keep = a.settings.int("dashboard_versions_keep") as i32;
    if let Err(e) = sqlx::query("DELETE FROM dashboard_versions WHERE dashboard_id = ? AND version <= ?").bind(id).bind(next - keep).execute(&mut *tx).await {
        return db_error(e);
    }
    if let Err(e) = tx.commit().await {
        return db_error(e);
    }
    db::audit(&a.db, &user.username, &user.ip, "dashboard.update", uid, "success", "info", message);
    Json(json!({"uid": uid, "version": next})).into_response()
}

pub async fn remove(State(a): S, user: User, Path(uid): Path<String>) -> Response {
    if let Err(r) = user.need("dashboards_delete") {
        return r;
    }
    match sqlx::query("DELETE FROM dashboards WHERE uid = ?").bind(&uid).execute(&a.db).await {
        Ok(r) if r.rows_affected() == 0 => problem(StatusCode::NOT_FOUND, "Not found", "No such dashboard."),
        Ok(_) => {
            db::audit(&a.db, &user.username, &user.ip, "dashboard.delete", &uid, "success", "notice", "");
            Json(json!({"ok": true})).into_response()
        }
        Err(e) => db_error(e),
    }
}

pub async fn versions(State(a): S, user: User, Path(uid): Path<String>) -> Response {
    if let Err(r) = user.need("dashboards_view") {
        return r;
    }
    match sqlx::query("SELECT v.version, v.message, v.created_by, v.created_at FROM dashboard_versions v JOIN dashboards d ON d.id = v.dashboard_id WHERE d.uid = ? ORDER BY v.version DESC").bind(&uid).fetch_all(&a.db).await {
        Ok(rows) => Json(json!(rows.iter().map(|r| json!({"version": r.get::<i32, _>("version"), "message": r.get::<String, _>("message"), "created_by": r.get::<String, _>("created_by"), "created_at": iso(r.get("created_at"))})).collect::<Vec<_>>())).into_response(),
        Err(e) => db_error(e),
    }
}

pub async fn restore(State(a): S, user: User, Path((uid, version)): Path<(String, i32)>) -> Response {
    if let Err(r) = user.need("dashboards_edit") {
        return r;
    }
    let body: Option<String> = match sqlx::query_scalar("SELECT v.body FROM dashboard_versions v JOIN dashboards d ON d.id = v.dashboard_id WHERE d.uid = ? AND v.version = ?").bind(&uid).bind(version).fetch_optional(&a.db).await {
        Ok(b) => b,
        Err(e) => return db_error(e),
    };
    let Some(body) = body else {
        return problem(StatusCode::NOT_FOUND, "Not found", "No such version.");
    };
    let d: Value = serde_json::from_str(&body).unwrap_or_else(|_| json!({}));
    save(&a, &user, &uid, d, None, &format!("Restored version {version}")).await
}

/// Brings back built-in dashboards that were deleted. Existing dashboards are left alone.
pub async fn restore_defaults(State(a): S, user: User) -> Response {
    if let Err(r) = user.need("dashboards_edit") {
        return r;
    }
    let mut restored = Vec::new();
    for d in built_in_dashboards() {
        let uid = d["uid"].as_str().unwrap_or("").to_string();
        let exists: Option<i64> = match sqlx::query_scalar("SELECT id FROM dashboards WHERE uid = ?").bind(&uid).fetch_optional(&a.db).await {
            Ok(v) => v,
            Err(e) => return db_error(e),
        };
        if exists.is_none() {
            if let Err(e) = db::insert_dashboard(&a.db, &d, &user.username).await {
                return db_error(e);
            }
            restored.push(uid);
        }
    }
    db::audit(&a.db, &user.username, &user.ip, "dashboard.restore_defaults", "", "success", "notice", &restored.join(","));
    Json(json!({"restored": restored})).into_response()
}

/// The saved SQL of one panel, so viewers can run a panel without being able to submit SQL.
pub async fn panel_sql(a: &App, uid: &str, panel: i64) -> Result<Option<Value>, Response> {
    let body: Option<String> = sqlx::query_scalar("SELECT body FROM dashboards WHERE uid = ?").bind(uid).fetch_optional(&a.db).await.map_err(db_error)?;
    let Some(body) = body else { return Ok(None) };
    let d: Value = serde_json::from_str(&body).unwrap_or_else(|_| json!({}));
    Ok(d["panels"].as_array().and_then(|ps| ps.iter().find(|p| p["id"].as_i64() == Some(panel)).cloned()))
}
