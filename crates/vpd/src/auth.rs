//! Sign-in, first-run setup, server-side sessions and the permission model.

use crate::api::{App, problem};
use crate::config;
use crate::db::{self, iso, utc_now};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng};
use argon2::Argon2;
use axum::extract::{ConnectInfo, FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Duration;
use rand::RngCore;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;

type S = State<Arc<App>>;

pub struct Perm {
    pub id: &'static str,
    pub group: &'static str,
    pub label: &'static str,
    pub help: &'static str,
}

const fn p(id: &'static str, group: &'static str, label: &'static str, help: &'static str) -> Perm {
    Perm { id, group, label, help }
}

/// Every permission here is enforced by the API. Don't add one that nothing checks.
pub const PERMISSIONS: &[Perm] = &[
    p("search_all", "Calls", "Browse all calls", "Search freely. Without it, a user must enter a number first."),
    p("subscribers_view", "Calls", "Subscriber view", "Open the Subscribers pages."),
    p("view_payload", "Calls", "See SIP payloads", "Without it, message headers and bodies are hidden."),
    p("unmask_numbers", "Calls", "Unmask numbers", "Without it, the other party's number is masked."),
    p("live_view", "Calls", "Live capture", "Open the live stream."),
    p("export_single", "Calls", "Export a call", "Download one call as PCAP, text or JSON."),
    p("export_bulk", "Calls", "Export results", "Download search results as CSV."),
    p("dashboards_view", "Dashboards", "View dashboards", "See dashboards and their data."),
    p("dashboards_edit", "Dashboards", "Edit dashboards", "Create, change, import and restore dashboards."),
    p("dashboards_delete", "Dashboards", "Delete dashboards", "Remove dashboards."),
    p("dashboards_sql", "Dashboards", "Write panel SQL", "Add or change SQL in dashboard panels."),
    p("explore", "Dashboards", "Explore", "Use the Explore page."),
    p("sql_workbench", "SQL", "SQL workbench", "Run read-only SQL."),
    p("users_manage", "Administration", "Manage users", "Create, change and remove users."),
    p("roles_manage", "Administration", "Manage roles", "Create, change and remove roles."),
    p("settings_manage", "Administration", "Manage settings", "Change application settings."),
    p("storage_manage", "Administration", "Manage storage", "Add, test and select storages."),
    p("system_view", "Administration", "View system pages", "See capture agents and system health."),
    p("audit_read", "Administration", "Read audit log", "See who did what."),
];

pub fn all_permissions() -> Vec<String> {
    PERMISSIONS.iter().map(|p| p.id.to_string()).collect()
}

/// The roles a new installation starts with: (name, description, permissions, locked). Only the
/// Administrator role is fixed; the others are starting points to edit, copy or delete.
pub fn built_in_roles() -> Vec<(&'static str, &'static str, Vec<String>, bool)> {
    let list = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    vec![
        ("Administrator", "Full access, including users, roles, settings and storage.", all_permissions(), true),
        ("Engineer", "Full troubleshooting: calls, SIP payloads, live capture, exports, dashboard editing, SQL and system pages.", list(&["search_all", "subscribers_view", "view_payload", "unmask_numbers", "live_view", "export_single", "export_bulk", "dashboards_view", "dashboards_edit", "dashboards_sql", "explore", "sql_workbench", "system_view"]), false),
        ("Analyst", "Monitors traffic and investigates calls. Dashboards, search and live capture, without editing or SQL.", list(&["search_all", "subscribers_view", "view_payload", "unmask_numbers", "live_view", "export_single", "dashboards_view", "explore"]), false),
        ("Support", "Looks up one subscriber at a time. The other party's number stays masked.", list(&["subscribers_view", "view_payload"]), false),
        ("Viewer", "Read-only dashboards.", list(&["dashboards_view"]), false),
        ("Auditor", "Read-only access to the audit log and system health.", list(&["audit_read", "system_view"]), false),
    ]
}

/// Renames and descriptions of the roles that earlier versions created, applied once to existing
/// installations: (old name, new name, old description).
pub const ROLE_RENAMES: &[(&str, &str, &str)] = &[
    ("Admin", "Administrator", "Everything, including users and settings"),
    ("Engineer", "Engineer", "All troubleshooting, editing dashboards, SQL"),
    ("NOC Operator", "Analyst", "Dashboards and search"),
    ("Support", "Support", "Helpdesk: looks up one number at a time, other party masked"),
    ("Auditor", "Auditor", "Read-only access to the audit log and system pages"),
];

pub fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default().hash_password(password.as_bytes(), &salt).map(|h| h.to_string()).map_err(|e| e.to_string())
}

fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
}

/// A valid hash of nothing, so that an unknown username costs the same time as a real one.
fn dummy_hash() -> &'static str {
    use std::sync::OnceLock;
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| hash_password("no such user").unwrap_or_default())
}

/// Stored permission lists. Names from older versions are dropped, so they can't block a save.
pub fn parse_list(s: &str) -> Vec<String> {
    let list: Vec<String> = serde_json::from_str(s).unwrap_or_default();
    list.into_iter().filter(|p| PERMISSIONS.iter().any(|k| k.id == p)).collect()
}

pub fn effective(role_perms: &[String], locked: bool, grants: &[String], denies: &[String]) -> HashSet<String> {
    let mut out: HashSet<String> = if locked { all_permissions().into_iter().collect() } else { role_perms.iter().cloned().collect() };
    out.extend(grants.iter().cloned());
    for d in denies {
        out.remove(d);
    }
    out
}

pub fn check_password_policy(app: &App, password: &str) -> Result<(), Response> {
    let min = app.settings.int("password_min_length") as usize;
    if password.chars().count() < min {
        return Err(problem(StatusCode::BAD_REQUEST, "Password too short", &format!("Use at least {min} characters.")));
    }
    Ok(())
}

pub fn client_ip(app: &App, headers: &HeaderMap, peer: Option<SocketAddr>) -> String {
    if app.boot.trust_proxy
        && let Some(v) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
        && let Some(first) = v.split(',').next()
    {
        return first.trim().to_string();
    }
    peer.map(|p| p.ip().to_string()).unwrap_or_default()
}

/// Compares secrets in time that doesn't depend on where they differ.
pub fn constant_eq(x: &str, y: &str) -> bool {
    let (x, y) = (Sha256::digest(x.as_bytes()), Sha256::digest(y.as_bytes()));
    x.iter().zip(y.iter()).fold(0u8, |acc, (p, q)| acc | (p ^ q)) == 0
}

fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|part| {
        let (k, v) = part.trim().split_once('=')?;
        (k == name).then(|| v.to_string())
    })
}

fn cookie_header(app: &App, headers: &HeaderMap, value: &str, max_age: i64) -> String {
    let secure = match app.settings.get("cookie_secure").as_str() {
        "true" => true,
        "false" => false,
        _ => headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()) == Some("https"),
    };
    format!("{}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{}", app.settings.get("cookie_name"), if secure { "; Secure" } else { "" })
}

pub struct User {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub perms: HashSet<String>,
    pub prefs: Value,
    pub ip: String,
    pub session: String,
}

impl User {
    pub fn can(&self, perm: &str) -> bool {
        self.perms.contains(perm)
    }

    pub fn need(&self, perm: &str) -> Result<(), Response> {
        if self.can(perm) {
            Ok(())
        } else {
            Err(problem(StatusCode::FORBIDDEN, "Not allowed", &format!("Your account does not have the \"{perm}\" permission.")))
        }
    }

    pub fn need_any(&self, perms: &[&str]) -> Result<(), Response> {
        if perms.iter().any(|p| self.can(p)) {
            Ok(())
        } else {
            Err(problem(StatusCode::FORBIDDEN, "Not allowed", "Your account does not have permission for this."))
        }
    }
}

fn unauthorized() -> Response {
    problem(StatusCode::UNAUTHORIZED, "Sign in required", "Your session has ended. Sign in again.")
}

impl FromRequestParts<Arc<App>> for User {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, app: &Arc<App>) -> Result<Self, Response> {
        let name = app.settings.get("cookie_name");
        let token = cookie_value(&parts.headers, &name).ok_or_else(unauthorized)?;
        let id = token_hash(&token);
        let row = sqlx::query(
            "SELECT s.user_id, s.created_at, s.last_seen_at, s.expires_at, u.username, u.display_name, u.status, u.grants, u.denies, u.prefs,
                    r.name AS role_name, r.permissions, r.locked
             FROM sessions s JOIN users u ON u.id = s.user_id JOIN roles r ON r.id = u.role_id WHERE s.id = ?",
        )
        .bind(&id)
        .fetch_optional(&app.db)
        .await
        .map_err(|e| {
            tracing::error!("session lookup failed: {e}");
            problem(StatusCode::SERVICE_UNAVAILABLE, "Database unavailable", "Try again in a moment.")
        })?
        .ok_or_else(unauthorized)?;

        let now = utc_now();
        let last_seen: chrono::NaiveDateTime = row.get("last_seen_at");
        let idle = Duration::minutes(app.settings.int("session_idle_minutes"));
        let expires: chrono::NaiveDateTime = row.get("expires_at");
        let status: String = row.get("status");
        if now > expires || now - last_seen > idle || status != "active" {
            let _ = sqlx::query("DELETE FROM sessions WHERE id = ?").bind(&id).execute(&app.db).await;
            return Err(unauthorized());
        }
        if now - last_seen > Duration::seconds(30) {
            let _ = sqlx::query("UPDATE sessions SET last_seen_at = ? WHERE id = ?").bind(now).bind(&id).execute(&app.db).await;
        }

        let role_perms = parse_list(&row.get::<String, _>("permissions"));
        let grants = parse_list(&row.get::<String, _>("grants"));
        let denies = parse_list(&row.get::<String, _>("denies"));
        let peer = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
        Ok(User {
            id: row.get("user_id"),
            username: row.get("username"),
            display_name: row.get("display_name"),
            role: row.get("role_name"),
            perms: effective(&role_perms, row.get::<i8, _>("locked") != 0, &grants, &denies),
            prefs: serde_json::from_str(&row.get::<String, _>("prefs")).unwrap_or_else(|_| json!({})),
            ip: client_ip(app, &parts.headers, peer),
            session: id,
        })
    }
}

async fn user_count(app: &App) -> Result<i64, Response> {
    sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&app.db).await.map_err(db_error)
}

pub fn db_error(e: sqlx::Error) -> Response {
    tracing::error!("database error: {e}");
    problem(StatusCode::INTERNAL_SERVER_ERROR, "Database error", "The request could not be completed. Check the server log.")
}

pub async fn state(State(a): S) -> Response {
    match user_count(&a).await {
        Ok(n) => Json(json!({
            "setup_required": n == 0,
            "setup_token_required": a.boot.setup_token.is_some(),
            "sso": crate::sso::login_options(&a),
            "settings": a.settings.public_json(),
        }))
        .into_response(),
        Err(r) => r,
    }
}

pub async fn start_session(a: &App, headers: &HeaderMap, user_id: i64, ip: &str) -> Result<String, Response> {
    let mut raw = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut raw);
    let token = hex::encode(raw);
    let hours = a.settings.int("session_absolute_hours");
    let now = utc_now();
    let ua: String = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("").chars().take(250).collect();
    sqlx::query("INSERT INTO sessions (id, user_id, created_at, last_seen_at, expires_at, ip, user_agent) VALUES (?, ?, ?, ?, ?, ?, ?)")
        .bind(token_hash(&token)).bind(user_id).bind(now).bind(now).bind(now + Duration::hours(hours)).bind(ip).bind(ua)
        .execute(&a.db).await.map_err(db_error)?;
    sqlx::query("UPDATE users SET last_login_at = ?, failed_attempts = 0, locked_until = NULL WHERE id = ?").bind(now).bind(user_id).execute(&a.db).await.map_err(db_error)?;
    Ok(cookie_header(a, headers, &token, hours * 3600))
}

#[derive(Deserialize)]
pub struct SetupBody {
    username: Option<String>,
    password: String,
    token: Option<String>,
}

/// First-run: the first person to open the app creates the admin account and is signed in.
pub async fn setup(State(a): S, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, Json(b): Json<SetupBody>) -> Response {
    let ip = client_ip(&a, &headers, Some(peer));
    if let Some(expected) = &a.boot.setup_token
        && !b.token.as_deref().is_some_and(|t| constant_eq(t, expected))
    {
        db::audit(&a.db, "-", &ip, "auth.setup", "admin", "failure", "warning", "bad setup token");
        return problem(StatusCode::FORBIDDEN, "Wrong setup token", "The setup token does not match SS_SETUP_TOKEN.");
    }
    if let Err(r) = check_password_policy(&a, &b.password) {
        return r;
    }
    let username = b.username.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(&a.boot.admin_username).to_string();
    let hash = match hash_password(&b.password) {
        Ok(h) => h,
        Err(e) => return problem(StatusCode::INTERNAL_SERVER_ERROR, "Could not hash password", &e),
    };
    let id = match create_first_admin(&a, &username, &hash).await {
        Ok(Some(id)) => id,
        Ok(None) => return problem(StatusCode::CONFLICT, "Already set up", "An account already exists. Sign in instead."),
        Err(r) => return r,
    };
    db::audit(&a.db, &username, &ip, "auth.setup", "admin", "success", "notice", "first admin created");
    match start_session(&a, &headers, id, &ip).await {
        Ok(cookie) => ([(header::SET_COOKIE, cookie)], Json(json!({"ok": true}))).into_response(),
        Err(r) => r,
    }
}

/// Creates the admin only if no user exists yet, under a lock so two requests can't both win.
async fn create_first_admin(a: &App, username: &str, hash: &str) -> Result<Option<i64>, Response> {
    let mut conn = a.db.acquire().await.map_err(db_error)?;
    sqlx::query("SELECT GET_LOCK('ss_setup', 10)").execute(&mut *conn).await.map_err(db_error)?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&mut *conn).await.map_err(db_error)?;
    let result = if count > 0 {
        None
    } else {
        let role: i64 = sqlx::query_scalar("SELECT id FROM roles WHERE locked = 1 ORDER BY id LIMIT 1").fetch_one(&mut *conn).await.map_err(db_error)?;
        let now = utc_now();
        let res = sqlx::query("INSERT INTO users (username, display_name, password_hash, role_id, grants, denies, prefs, created_at, updated_at) VALUES (?, ?, ?, ?, '[]', '[]', '{}', ?, ?)")
            .bind(username).bind("Administrator").bind(hash).bind(role).bind(now).bind(now)
            .execute(&mut *conn).await.map_err(db_error)?;
        Some(res.last_insert_id() as i64)
    };
    sqlx::query("SELECT RELEASE_LOCK('ss_setup')").execute(&mut *conn).await.map_err(db_error)?;
    Ok(result)
}

/// With SS_ADMIN_PASSWORD set, the admin is created at startup and the setup screen is skipped.
pub async fn bootstrap_admin(a: &App) -> Result<(), String> {
    let Some(password) = a.boot.admin_password.clone() else { return Ok(()) };
    let hash = hash_password(&password)?;
    match create_first_admin(a, &a.boot.admin_username, &hash).await {
        Ok(Some(_)) => tracing::info!("created admin account {} from SS_ADMIN_PASSWORD", a.boot.admin_username),
        Ok(None) => {}
        Err(_) => return Err("could not create the admin account".into()),
    }
    Ok(())
}

/// Failed sign-ins per client address in the current window. Account lockout stops guessing one
/// password; this stops one address from trying many accounts.
static FAILURES: std::sync::Mutex<Option<std::collections::HashMap<String, (u32, std::time::Instant)>>> = std::sync::Mutex::new(None);
const FAILURE_WINDOW: std::time::Duration = std::time::Duration::from_secs(600);
const FAILURES_PER_ADDRESS: u32 = 30;

fn too_many_failures(ip: &str) -> bool {
    let mut guard = FAILURES.lock().unwrap();
    let map = guard.get_or_insert_with(Default::default);
    map.retain(|_, (_, since)| since.elapsed() < FAILURE_WINDOW);
    map.get(ip).is_some_and(|(n, _)| *n >= FAILURES_PER_ADDRESS)
}

fn record_failure(ip: &str) {
    let mut guard = FAILURES.lock().unwrap();
    let entry = guard.get_or_insert_with(Default::default).entry(ip.to_string()).or_insert((0, std::time::Instant::now()));
    entry.0 += 1;
}

#[derive(Deserialize)]
pub struct LoginBody {
    username: String,
    password: String,
}

pub async fn login(State(a): S, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, Json(b): Json<LoginBody>) -> Response {
    let ip = client_ip(&a, &headers, Some(peer));
    if too_many_failures(&ip) {
        db::audit(&a.db, b.username.trim(), &ip, "auth.login", "session", "failure", "warning", "too many failures from this address");
        return problem(StatusCode::TOO_MANY_REQUESTS, "Too many attempts", "Too many failed sign-ins from this address. Try again in a few minutes.");
    }
    let bad = || {
        record_failure(&ip);
        problem(StatusCode::UNAUTHORIZED, "Sign-in failed", "Wrong username or password.")
    };
    let row = match sqlx::query(
        "SELECT u.id, u.password_hash, u.auth_source, u.status, u.failed_attempts, u.locked_until, u.grants, u.denies, r.permissions, r.locked
         FROM users u JOIN roles r ON r.id = u.role_id WHERE u.username = ?",
    ).bind(b.username.trim()).fetch_optional(&a.db).await {
        Ok(r) => r,
        Err(e) => return db_error(e),
    };
    let Some(row) = row else {
        let _ = verify_password(&b.password, dummy_hash());
        db::audit(&a.db, b.username.trim(), &ip, "auth.login", "session", "failure", "warning", "unknown user");
        return bad();
    };
    let id: i64 = row.get("id");
    let locked_until: Option<chrono::NaiveDateTime> = row.get("locked_until");
    if locked_until.is_some_and(|t| t > utc_now()) {
        db::audit(&a.db, b.username.trim(), &ip, "auth.login", "session", "failure", "warning", "account locked");
        return problem(StatusCode::TOO_MANY_REQUESTS, "Account locked", "Too many failed sign-ins. Try again later.");
    }
    if row.get::<String, _>("status") != "active" {
        db::audit(&a.db, b.username.trim(), &ip, "auth.login", "session", "failure", "warning", "account disabled");
        return bad();
    }
    if !verify_password(&b.password, &row.get::<String, _>("password_hash")) {
        let attempts = row.get::<i32, _>("failed_attempts") + 1;
        let lock = (attempts as i64 >= a.settings.int("login_max_attempts")).then(|| utc_now() + Duration::minutes(a.settings.int("login_lockout_minutes")));
        let _ = sqlx::query("UPDATE users SET failed_attempts = ?, locked_until = ? WHERE id = ?").bind(if lock.is_some() { 0 } else { attempts }).bind(lock).bind(id).execute(&a.db).await;
        db::audit(&a.db, b.username.trim(), &ip, "auth.login", "session", "failure", "warning", "wrong password");
        return bad();
    }
    // With single sign-on in charge, a password only still works for those who can manage users,
    // as a way in when the provider is down.
    if crate::sso::enabled(&a) && !a.settings.bool("sso_allow_password") {
        let perms = effective(&parse_list(&row.get::<String, _>("permissions")), row.get::<i8, _>("locked") != 0, &parse_list(&row.get::<String, _>("grants")), &parse_list(&row.get::<String, _>("denies")));
        if !perms.contains("users_manage") {
            db::audit(&a.db, b.username.trim(), &ip, "auth.login", "session", "failure", "warning", "password sign-in is off");
            return problem(StatusCode::FORBIDDEN, "Use single sign-on", "Sign in with the single sign-on button.");
        }
    }
    db::audit(&a.db, b.username.trim(), &ip, "auth.login", "session", "success", "info", "");
    match start_session(&a, &headers, id, &ip).await {
        Ok(cookie) => ([(header::SET_COOKIE, cookie)], Json(json!({"ok": true}))).into_response(),
        Err(r) => r,
    }
}

pub async fn logout(State(a): S, headers: HeaderMap, user: User) -> Response {
    let _ = sqlx::query("DELETE FROM sessions WHERE id = ?").bind(&user.session).execute(&a.db).await;
    db::audit(&a.db, &user.username, &user.ip, "auth.logout", "session", "success", "info", "");
    ([(header::SET_COOKIE, cookie_header(&a, &headers, "", 0))], Json(json!({"ok": true}))).into_response()
}

pub async fn me(State(a): S, user: User) -> Json<Value> {
    Json(json!({
        "id": user.id, "username": user.username, "display_name": user.display_name, "role": user.role,
        "permissions": user.perms.iter().collect::<Vec<_>>(), "prefs": user.prefs,
        "settings": a.settings.public_json(),
    }))
}

pub async fn put_prefs(State(a): S, user: User, Json(prefs): Json<Value>) -> Response {
    if !prefs.is_object() || prefs.to_string().len() > 16_384 {
        return problem(StatusCode::BAD_REQUEST, "Invalid preferences", "Preferences must be a JSON object under 16 KB.");
    }
    match sqlx::query("UPDATE users SET prefs = ? WHERE id = ?").bind(prefs.to_string()).bind(user.id).execute(&a.db).await {
        Ok(_) => Json(prefs).into_response(),
        Err(e) => db_error(e),
    }
}

#[derive(Deserialize)]
pub struct ChangePassword {
    current: String,
    new: String,
}

pub async fn change_password(State(a): S, user: User, Json(b): Json<ChangePassword>) -> Response {
    let hash: String = match sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?").bind(user.id).fetch_one(&a.db).await {
        Ok(h) => h,
        Err(e) => return db_error(e),
    };
    if hash == "!" {
        return problem(StatusCode::CONFLICT, "Signed in with single sign-on", "This account has no password here. Change it with your sign-in provider.");
    }
    if !verify_password(&b.current, &hash) {
        return problem(StatusCode::BAD_REQUEST, "Wrong password", "The current password is not correct.");
    }
    if let Err(r) = check_password_policy(&a, &b.new) {
        return r;
    }
    let new_hash = match hash_password(&b.new) {
        Ok(h) => h,
        Err(e) => return problem(StatusCode::INTERNAL_SERVER_ERROR, "Could not hash password", &e),
    };
    if let Err(e) = sqlx::query("UPDATE users SET password_hash = ?, updated_at = ? WHERE id = ?").bind(new_hash).bind(utc_now()).bind(user.id).execute(&a.db).await {
        return db_error(e);
    }
    let _ = sqlx::query("DELETE FROM sessions WHERE user_id = ? AND id <> ?").bind(user.id).bind(&user.session).execute(&a.db).await;
    db::audit(&a.db, &user.username, &user.ip, "user.password_change", &user.username, "success", "notice", "");
    Json(json!({"ok": true})).into_response()
}

pub async fn permissions(user: User) -> Response {
    if let Err(r) = user.need_any(&["users_manage", "roles_manage"]) {
        return r;
    }
    Json(json!(PERMISSIONS.iter().map(|p| json!({"id": p.id, "group": p.group, "label": p.label, "help": p.help})).collect::<Vec<_>>())).into_response()
}

// Roles

fn role_json(r: &sqlx::mysql::MySqlRow) -> Value {
    let locked = r.get::<i8, _>("locked") != 0;
    json!({
        "id": r.get::<i64, _>("id"), "name": r.get::<String, _>("name"), "description": r.get::<String, _>("description"),
        "permissions": if locked { all_permissions() } else { parse_list(&r.get::<String, _>("permissions")) },
        "locked": locked, "users": r.get::<i64, _>("users"),
    })
}

pub async fn list_roles(State(a): S, user: User) -> Response {
    if let Err(r) = user.need_any(&["users_manage", "roles_manage"]) {
        return r;
    }
    match sqlx::query("SELECT r.*, (SELECT COUNT(*) FROM users u WHERE u.role_id = r.id) AS users FROM roles r ORDER BY r.locked DESC, r.name").fetch_all(&a.db).await {
        Ok(rows) => Json(json!(rows.iter().map(role_json).collect::<Vec<_>>())).into_response(),
        Err(e) => db_error(e),
    }
}

#[derive(Deserialize)]
pub struct RoleBody {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    permissions: Vec<String>,
}

/// Nobody can hand out a permission they don't hold themselves, so managing users or roles
/// can't be turned into more access than the manager already has.
fn within_own(user: &User, list: &[String]) -> Result<(), Response> {
    match list.iter().find(|p| !user.can(p)) {
        Some(p) => Err(problem(StatusCode::FORBIDDEN, "Not allowed", &format!("You can't grant \"{p}\" because your own account doesn't have it."))),
        None => Ok(()),
    }
}

/// The permissions a role gives, for checking an assignment against the assigner's own.
async fn role_permissions(a: &App, role_id: i64) -> Result<Vec<String>, Response> {
    let row = sqlx::query("SELECT permissions, locked FROM roles WHERE id = ?").bind(role_id).fetch_optional(&a.db).await.map_err(db_error)?;
    let row = row.ok_or_else(|| problem(StatusCode::BAD_REQUEST, "Unknown role", "Pick one of the existing roles."))?;
    Ok(if row.get::<i8, _>("locked") != 0 { all_permissions() } else { parse_list(&row.get::<String, _>("permissions")) })
}

fn clean_name(name: &str, what: &str) -> Result<String, Response> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(problem(StatusCode::BAD_REQUEST, "Invalid name", &format!("Give the {what} a name of 1 to 64 characters.")));
    }
    Ok(name.to_string())
}

fn known_perms(list: &[String]) -> Result<(), Response> {
    let known = all_permissions();
    match list.iter().find(|p| !known.contains(p)) {
        Some(p) => Err(problem(StatusCode::BAD_REQUEST, "Unknown permission", p)),
        None => Ok(()),
    }
}

pub async fn create_role(State(a): S, user: User, Json(b): Json<RoleBody>) -> Response {
    if let Err(r) = user.need("roles_manage").and_then(|_| known_perms(&b.permissions)).and_then(|_| within_own(&user, &b.permissions)) {
        return r;
    }
    let name = match clean_name(&b.name, "role") {
        Ok(n) => n,
        Err(r) => return r,
    };
    let name = name.as_str();
    let res = sqlx::query("INSERT INTO roles (name, description, permissions, created_at, updated_at) VALUES (?, ?, ?, ?, ?)")
        .bind(name).bind(b.description.trim()).bind(json!(b.permissions).to_string()).bind(utc_now()).bind(utc_now()).execute(&a.db).await;
    match res {
        Ok(r) => {
            db::audit(&a.db, &user.username, &user.ip, "role.create", name, "success", "notice", &b.permissions.join(","));
            Json(json!({"id": r.last_insert_id()})).into_response()
        }
        Err(e) if is_duplicate(&e) => problem(StatusCode::CONFLICT, "Name in use", "A role with this name already exists."),
        Err(e) => db_error(e),
    }
}

pub fn is_duplicate(e: &sqlx::Error) -> bool {
    e.as_database_error().is_some_and(|d| d.is_unique_violation())
}

pub async fn update_role(State(a): S, user: User, Path(id): Path<i64>, Json(b): Json<RoleBody>) -> Response {
    if let Err(r) = user.need("roles_manage").and_then(|_| known_perms(&b.permissions)) {
        return r;
    }
    let name = match clean_name(&b.name, "role") {
        Ok(n) => n,
        Err(r) => return r,
    };
    let locked: Option<i8> = match sqlx::query_scalar("SELECT locked FROM roles WHERE id = ?").bind(id).fetch_optional(&a.db).await {
        Ok(v) => v,
        Err(e) => return db_error(e),
    };
    match locked {
        None => return problem(StatusCode::NOT_FOUND, "Not found", "No such role."),
        Some(1) => return problem(StatusCode::FORBIDDEN, "Role is locked", "The Administrator role always has every permission, so nobody can lock themselves out."),
        _ => {}
    }
    // Taking away permissions is always allowed; a manager just can't add ones they lack.
    let before = match role_permissions(&a, id).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let added: Vec<String> = b.permissions.iter().filter(|p| !before.contains(p)).cloned().collect();
    if let Err(r) = within_own(&user, &added) {
        return r;
    }
    let mut tx = match a.db.begin().await {
        Ok(t) => t,
        Err(e) => return db_error(e),
    };
    let result = sqlx::query("UPDATE roles SET name = ?, description = ?, permissions = ?, updated_at = ? WHERE id = ?")
        .bind(&name).bind(b.description.trim()).bind(json!(b.permissions).to_string()).bind(utc_now()).bind(id).execute(&mut *tx).await;
    if let Err(e) = result {
        return if is_duplicate(&e) { problem(StatusCode::CONFLICT, "Name in use", "A role with this name already exists.") } else { db_error(e) };
    }
    match has_manager(&mut tx).await {
        Ok(true) => {}
        Ok(false) => return problem(StatusCode::CONFLICT, "Last administrator", "This would leave nobody able to manage users, so nothing was changed."),
        Err(e) => return db_error(e),
    }
    match tx.commit().await {
        Ok(_) => {
            db::audit(&a.db, &user.username, &user.ip, "role.update", &name, "success", "notice", &b.permissions.join(","));
            Json(json!({"ok": true})).into_response()
        }
        Err(e) if is_duplicate(&e) => problem(StatusCode::CONFLICT, "Name in use", "A role with this name already exists."),
        Err(e) => db_error(e),
    }
}

pub async fn delete_role(State(a): S, user: User, Path(id): Path<i64>) -> Response {
    if let Err(r) = user.need("roles_manage") {
        return r;
    }
    let row = match sqlx::query("SELECT name, locked, (SELECT COUNT(*) FROM users u WHERE u.role_id = r.id) AS users FROM roles r WHERE id = ?").bind(id).fetch_optional(&a.db).await {
        Ok(Some(r)) => r,
        Ok(None) => return problem(StatusCode::NOT_FOUND, "Not found", "No such role."),
        Err(e) => return db_error(e),
    };
    if row.get::<i8, _>("locked") != 0 {
        return problem(StatusCode::FORBIDDEN, "Role is locked", "The Admin role can't be deleted.");
    }
    if row.get::<i64, _>("users") > 0 {
        return problem(StatusCode::CONFLICT, "Role in use", "Move its users to another role first.");
    }
    match sqlx::query("DELETE FROM roles WHERE id = ?").bind(id).execute(&a.db).await {
        Ok(_) => {
            db::audit(&a.db, &user.username, &user.ip, "role.delete", &row.get::<String, _>("name"), "success", "notice", "");
            Json(json!({"ok": true})).into_response()
        }
        Err(e) => db_error(e),
    }
}

// Users

fn user_json(r: &sqlx::mysql::MySqlRow) -> Value {
    let last: Option<chrono::NaiveDateTime> = r.get("last_login_at");
    let locked: Option<chrono::NaiveDateTime> = r.get("locked_until");
    json!({
        "id": r.get::<i64, _>("id"), "username": r.get::<String, _>("username"), "display_name": r.get::<String, _>("display_name"),
        "email": r.get::<String, _>("email"), "role_id": r.get::<i64, _>("role_id"), "role": r.get::<String, _>("role_name"),
        "status": r.get::<String, _>("status"), "grants": parse_list(&r.get::<String, _>("grants")), "denies": parse_list(&r.get::<String, _>("denies")),
        "last_login": last.map(iso), "locked": locked.is_some_and(|t| t > utc_now()),
        "sso": r.get::<String, _>("auth_source") == "sso", "linked": r.get::<Option<String>, _>("external_id").is_some(),
    })
}

pub async fn list_users(State(a): S, user: User) -> Response {
    if let Err(r) = user.need("users_manage") {
        return r;
    }
    match sqlx::query("SELECT u.*, r.name AS role_name FROM users u JOIN roles r ON r.id = u.role_id ORDER BY u.username").fetch_all(&a.db).await {
        Ok(rows) => Json(json!(rows.iter().map(user_json).collect::<Vec<_>>())).into_response(),
        Err(e) => db_error(e),
    }
}

#[derive(Deserialize)]
pub struct UserBody {
    username: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    password: Option<String>,
    role_id: i64,
    #[serde(default = "active")]
    status: String,
    #[serde(default)]
    grants: Vec<String>,
    #[serde(default)]
    denies: Vec<String>,
    /// Signs in with single sign-on only, and has no password here.
    #[serde(default)]
    sso: bool,
}

fn active() -> String {
    "active".into()
}

impl UserBody {
    fn check(&self) -> Result<String, Response> {
        known_perms(&self.grants)?;
        known_perms(&self.denies)?;
        if !matches!(self.status.as_str(), "active" | "disabled") {
            return Err(problem(StatusCode::BAD_REQUEST, "Invalid status", "Status must be active or disabled."));
        }
        if self.email.len() > 190 || self.display_name.chars().count() > 128 {
            return Err(problem(StatusCode::BAD_REQUEST, "Too long", "Display name or email is too long."));
        }
        let username = self.username.trim();
        if username.is_empty() || username.chars().count() > 96 || username.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(problem(StatusCode::BAD_REQUEST, "Invalid username", "Use 1 to 96 characters without spaces."));
        }
        Ok(username.to_string())
    }
}

/// What the account would be able to do, for comparing with the person changing it.
async fn assigned_permissions(a: &App, role_id: i64, grants: &[String]) -> Result<Vec<String>, Response> {
    let mut perms = role_permissions(a, role_id).await?;
    perms.extend(grants.iter().cloned());
    Ok(perms)
}

/// A user manager may only change accounts that have no more access than they have, so a
/// partial administrator can't reset the password of a full one and sign in as them.
async fn target_within(a: &App, user: &User, id: i64) -> Result<(), Response> {
    let row = sqlx::query("SELECT u.grants, u.denies, r.permissions, r.locked FROM users u JOIN roles r ON r.id = u.role_id WHERE u.id = ?")
        .bind(id).fetch_optional(&a.db).await.map_err(db_error)?
        .ok_or_else(|| problem(StatusCode::NOT_FOUND, "Not found", "No such user."))?;
    let theirs = effective(&parse_list(&row.get::<String, _>("permissions")), row.get::<i8, _>("locked") != 0, &parse_list(&row.get::<String, _>("grants")), &parse_list(&row.get::<String, _>("denies")));
    if theirs.iter().all(|p| user.can(p)) {
        Ok(())
    } else {
        Err(problem(StatusCode::FORBIDDEN, "Not allowed", "This account has permissions that yours doesn't, so only someone with at least the same access can change it."))
    }
}

/// True when at least one active account can manage users. Run inside the transaction making a
/// change, so the change can be rolled back rather than repaired afterwards.
async fn has_manager(tx: &mut sqlx::MySqlConnection) -> Result<bool, sqlx::Error> {
    let rows = sqlx::query("SELECT u.grants, u.denies, r.permissions, r.locked FROM users u JOIN roles r ON r.id = u.role_id WHERE u.status = 'active'").fetch_all(tx).await?;
    Ok(rows.iter().any(|r| {
        effective(&parse_list(&r.get::<String, _>("permissions")), r.get::<i8, _>("locked") != 0, &parse_list(&r.get::<String, _>("grants")), &parse_list(&r.get::<String, _>("denies"))).contains("users_manage")
    }))
}

pub async fn create_user(State(a): S, user: User, Json(b): Json<UserBody>) -> Response {
    let username = match user.need("users_manage").and_then(|_| b.check()) {
        Ok(u) => u,
        Err(r) => return r,
    };
    if let Err(r) = async { within_own(&user, &assigned_permissions(&a, b.role_id, &b.grants).await?) }.await {
        return r;
    }
    let username = username.as_str();
    let hash = if b.sso {
        "!".to_string()
    } else {
        let Some(password) = b.password.as_deref().filter(|p| !p.is_empty()) else {
            return problem(StatusCode::BAD_REQUEST, "Password required", "Set a password for the new user.");
        };
        if let Err(r) = check_password_policy(&a, password) {
            return r;
        }
        match hash_password(password) {
            Ok(h) => h,
            Err(e) => return problem(StatusCode::INTERNAL_SERVER_ERROR, "Could not hash password", &e),
        }
    };
    let now = utc_now();
    let res = sqlx::query("INSERT INTO users (username, display_name, email, password_hash, auth_source, role_id, status, grants, denies, prefs, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, '{}', ?, ?)")
        .bind(username).bind(b.display_name.trim()).bind(b.email.trim()).bind(hash).bind(if b.sso { "sso" } else { "local" }).bind(b.role_id).bind(&b.status)
        .bind(json!(b.grants).to_string()).bind(json!(b.denies).to_string()).bind(now).bind(now)
        .execute(&a.db).await;
    match res {
        Ok(r) => {
            db::audit(&a.db, &user.username, &user.ip, "user.create", username, "success", "notice", "");
            Json(json!({"id": r.last_insert_id()})).into_response()
        }
        Err(e) if is_duplicate(&e) => problem(StatusCode::CONFLICT, "Username in use", "A user with this name already exists."),
        Err(e) => db_error(e),
    }
}

pub async fn update_user(State(a): S, user: User, Path(id): Path<i64>, Json(b): Json<UserBody>) -> Response {
    let username = match user.need("users_manage").and_then(|_| b.check()) {
        Ok(u) => u,
        Err(r) => return r,
    };
    if let Err(r) = async {
        target_within(&a, &user, id).await?;
        within_own(&user, &assigned_permissions(&a, b.role_id, &b.grants).await?)
    }
    .await
    {
        return r;
    }
    let mut tx = match a.db.begin().await {
        Ok(t) => t,
        Err(e) => return db_error(e),
    };
    let result = sqlx::query("UPDATE users SET username = ?, display_name = ?, email = ?, role_id = ?, status = ?, grants = ?, denies = ?, updated_at = ? WHERE id = ?")
        .bind(&username).bind(b.display_name.trim()).bind(b.email.trim()).bind(b.role_id).bind(&b.status)
        .bind(json!(b.grants).to_string()).bind(json!(b.denies).to_string()).bind(utc_now()).bind(id)
        .execute(&mut *tx).await;
    if let Err(e) = result {
        return if is_duplicate(&e) { problem(StatusCode::CONFLICT, "Username in use", "A user with this name already exists.") } else { db_error(e) };
    }
    match has_manager(&mut tx).await {
        Ok(true) => {}
        Ok(false) => return problem(StatusCode::CONFLICT, "Last administrator", "This would leave nobody able to manage users, so nothing was changed."),
        Err(e) => return db_error(e),
    }
    if let Err(e) = tx.commit().await {
        return db_error(e);
    }
    if b.status != "active" {
        let _ = sqlx::query("DELETE FROM sessions WHERE user_id = ?").bind(id).execute(&a.db).await;
    }
    db::audit(&a.db, &user.username, &user.ip, "user.update", &username, "success", "notice", &format!("role_id={} status={} grants={} denies={}", b.role_id, b.status, b.grants.join(","), b.denies.join(",")));
    Json(json!({"ok": true})).into_response()
}

#[derive(Deserialize)]
pub struct ResetBody {
    password: String,
}

pub async fn reset_password(State(a): S, user: User, Path(id): Path<i64>, Json(b): Json<ResetBody>) -> Response {
    if let Err(r) = user.need("users_manage").and_then(|_| check_password_policy(&a, &b.password)) {
        return r;
    }
    if let Err(r) = target_within(&a, &user, id).await {
        return r;
    }
    match sqlx::query_scalar::<_, String>("SELECT auth_source FROM users WHERE id = ?").bind(id).fetch_optional(&a.db).await {
        Ok(Some(source)) if source == "sso" => return problem(StatusCode::CONFLICT, "Single sign-on account", "This account signs in with single sign-on and has no password here."),
        Ok(_) => {}
        Err(e) => return db_error(e),
    }
    let hash = match hash_password(&b.password) {
        Ok(h) => h,
        Err(e) => return problem(StatusCode::INTERNAL_SERVER_ERROR, "Could not hash password", &e),
    };
    if let Err(e) = sqlx::query("UPDATE users SET password_hash = ?, failed_attempts = 0, locked_until = NULL, updated_at = ? WHERE id = ?").bind(hash).bind(utc_now()).bind(id).execute(&a.db).await {
        return db_error(e);
    }
    let _ = sqlx::query("DELETE FROM sessions WHERE user_id = ?").bind(id).execute(&a.db).await;
    db::audit(&a.db, &user.username, &user.ip, "user.password_reset", &id.to_string(), "success", "notice", "");
    Json(json!({"ok": true})).into_response()
}

pub async fn delete_user(State(a): S, user: User, Path(id): Path<i64>) -> Response {
    if let Err(r) = user.need("users_manage") {
        return r;
    }
    if id == user.id {
        return problem(StatusCode::CONFLICT, "Can't delete yourself", "Ask another administrator to remove this account.");
    }
    if let Err(r) = target_within(&a, &user, id).await {
        return r;
    }
    let name: Option<String> = sqlx::query_scalar("SELECT username FROM users WHERE id = ?").bind(id).fetch_optional(&a.db).await.ok().flatten();
    let _ = sqlx::query("DELETE FROM saved_searches WHERE owner_id = ?").bind(id).execute(&a.db).await;
    match sqlx::query("DELETE FROM users WHERE id = ?").bind(id).execute(&a.db).await {
        Ok(_) => {
            db::audit(&a.db, &user.username, &user.ip, "user.delete", name.as_deref().unwrap_or(""), "success", "notice", "");
            Json(json!({"ok": true})).into_response()
        }
        Err(e) => db_error(e),
    }
}

// Settings

pub async fn get_settings(State(a): S, user: User) -> Response {
    if let Err(r) = user.need("settings_manage") {
        return r;
    }
    Json(a.settings.admin_json()).into_response()
}

#[derive(Deserialize)]
pub struct SettingBody {
    value: Option<String>,
}

pub async fn put_setting(State(a): S, user: User, Path(key): Path<String>, Json(b): Json<SettingBody>) -> Response {
    if let Err(r) = user.need("settings_manage") {
        return r;
    }
    let Some(def) = config::def(&key) else {
        return problem(StatusCode::NOT_FOUND, "Unknown setting", &key);
    };
    if a.settings.locked(&key) {
        return problem(StatusCode::CONFLICT, "Set by environment", &format!("{} is set in the environment, which takes priority.", config::env_name(&key)));
    }
    let mut stored_value = None;
    let result = match &b.value {
        Some(value) => {
            if let Err(e) = config::validate(def, value) {
                return problem(StatusCode::BAD_REQUEST, "Invalid value", &format!("{} {e}", def.label));
            }
            let stored = if def.secret && !value.trim().is_empty() { crate::storage::seal(&a, value.trim()) } else { value.trim().to_string() };
            stored_value = Some(stored.clone());
            sqlx::query("REPLACE INTO settings (skey, value, updated_by, updated_at) VALUES (?, ?, ?, ?)").bind(&key).bind(stored).bind(&user.username).bind(utc_now()).execute(&a.db).await
        }
        None => sqlx::query("DELETE FROM settings WHERE skey = ?").bind(&key).execute(&a.db).await,
    };
    if let Err(e) = result {
        return db_error(e);
    }
    a.settings.set_db(&key, stored_value.as_deref());
    let shown = match &b.value {
        None => "(reset to default)",
        Some(_) if def.secret => "(secret changed)",
        Some(v) => v.as_str(),
    };
    db::audit(&a.db, &user.username, &user.ip, "settings.update", &key, "success", "notice", shown);
    Json(json!({"ok": true})).into_response()
}
