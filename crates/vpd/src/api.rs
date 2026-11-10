//! REST API under /api/v1, the live feed, health checks and the embedded single-page app.

use crate::auth::{self, User, db_error};
use crate::callview;
use crate::ch::Ch;
use crate::config::{Boot, Settings};
use crate::dashboards;
use crate::db::{self, iso, utc_now};
use crate::flow;
use crate::ingest::Ingest;
use crate::metrics::{self, Buckets, Opts, parse_time, parse_tz, sql_literal_time};
use crate::model::*;
use crate::registrations as reg;
use crate::search;
use crate::sql;
use crate::storage;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use futures_util::future::join_all;
use rust_embed::RustEmbed;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use sqlx::mysql::MySqlPool;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::broadcast::error::TryRecvError;

#[derive(RustEmbed)]
#[folder = "../../web/dist/"]
struct Assets;

pub struct App {
    pub boot: Boot,
    pub settings: Settings,
    pub db: MySqlPool,
    pub storage: RwLock<Option<Ch>>,
    /// The selected storage is being started or prepared.
    pub storage_starting: AtomicBool,
    /// Bumped on every storage change, so a slow start can't overwrite a newer choice.
    pub storage_generation: AtomicU64,
    pub local: crate::local::Engine,
    pub ingest: Ingest,
    pub live_sessions: AtomicUsize,
}

type S = State<Arc<App>>;

impl App {
    fn opts(&self) -> Opts {
        Opts {
            settle_us: self.settings.int("settle_window_seconds") * SEC,
            max_points: self.settings.int("chart_max_points") as usize,
            top_series: self.settings.int("chart_top_series") as usize,
            timeout_s: self.settings.int("sql_timeout_seconds") as u64,
            max_rows: self.settings.int("sql_max_rows") as usize,
        }
    }

    /// The selected ClickHouse, or a response that tells the user to select one.
    fn ch(&self) -> Result<Ch, Response> {
        self.storage.read().unwrap().clone().ok_or_else(|| {
            if self.storage_starting.load(Ordering::SeqCst) {
                problem(StatusCode::SERVICE_UNAVAILABLE, "Storage is starting", "Data appears in a few seconds.")
            } else {
                problem(StatusCode::SERVICE_UNAVAILABLE, "No storage selected", "Select a storage on the Storage page.")
            }
        })
    }
}


pub fn now_us() -> i64 {
    chrono::Utc::now().timestamp_micros()
}

/// Problem Details response (RFC 9457).
pub fn problem(status: StatusCode, title: &str, detail: &str) -> Response {
    let body = json!({"type": "about:blank", "title": title, "status": status.as_u16(), "detail": detail});
    (status, [(header::CONTENT_TYPE, "application/problem+json")], body.to_string()).into_response()
}

fn not_found(what: &str) -> Response {
    problem(StatusCode::NOT_FOUND, "Not found", what)
}

/// Browsers can't add a custom header to a cross-site request without CORS, so requiring one on
/// every write stops cross-site request forgery.
async fn csrf(request: Request, next: Next) -> Response {
    let safe = matches!(*request.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if !safe && !request.headers().contains_key("x-ss-request") {
        return problem(StatusCode::FORBIDDEN, "Blocked", "This request is missing the X-SS-Request header.");
    }
    next.run(request).await
}

async fn security_headers(request: Request, next: Next) -> Response {
    let https = request.headers().get("x-forwarded-proto").and_then(|v| v.to_str().ok()) == Some("https");
    let mut res = next.run(request).await;
    let h = res.headers_mut();
    if https {
        h.insert("strict-transport-security", HeaderValue::from_static("max-age=31536000"));
    }
    h.insert("cross-origin-opener-policy", HeaderValue::from_static("same-origin"));
    h.insert("permissions-policy", HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=()"));
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert("referrer-policy", HeaderValue::from_static("same-origin"));
    h.insert("content-security-policy", HeaderValue::from_static("default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src 'self'; frame-ancestors 'none'"));
    res
}

pub fn router(app: Arc<App>) -> Router {
    let api = Router::new()
        .route("/auth/state", get(auth::state))
        .route("/auth/setup", post(auth::setup))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/sso/login", get(crate::sso::start))
        .route("/auth/sso/callback", get(crate::sso::callback))
        .route("/admin/sso/check", post(crate::sso::check))
        .route("/auth/me", get(auth::me))
        .route("/auth/me/prefs", put(auth::put_prefs))
        .route("/auth/me/password", post(auth::change_password))
        .route("/permissions", get(auth::permissions))
        .route("/roles", get(auth::list_roles).post(auth::create_role))
        .route("/roles/{id}", put(auth::update_role).delete(auth::delete_role))
        .route("/users", get(auth::list_users).post(auth::create_user))
        .route("/users/{id}", put(auth::update_user).delete(auth::delete_user))
        .route("/users/{id}/password", post(auth::reset_password))
        .route("/settings", get(auth::get_settings))
        .route("/settings/{key}", put(auth::put_setting))
        .route("/storages", get(storage::list).post(storage::create))
        .route("/storages/active", get(storage::active))
        .route("/storages/{id}", put(storage::update).delete(storage::remove))
        .route("/storages/{id}/test", post(storage::test))
        .route("/storages/{id}/activate", post(storage::activate))
        .route("/meta", get(meta))
        .route("/metrics/catalogue", get(catalogue))
        .route("/metrics/query", post(metric_query))
        .route("/fields/{field}/values", get(field_values))
        .route("/dashboards", get(dashboards::list).post(dashboards::create))
        .route("/dashboards/restore-defaults", post(dashboards::restore_defaults))
        .route("/dashboards/{uid}", get(dashboards::get).put(dashboards::update).delete(dashboards::remove))
        .route("/dashboards/{uid}/versions", get(dashboards::versions))
        .route("/dashboards/{uid}/restore/{version}", post(dashboards::restore))
        .route("/calls/search", get(call_search))
        .route("/calls/export.csv", get(export_csv))
        .route("/calls/by-callid/{id}", get(by_call_id))
        .route("/calls/{uid}", get(call_get))
        .route("/calls/{uid}/flow", get(call_flow))
        .route("/calls/{uid}/media", get(call_media))
        .route("/calls/{uid}/logs", get(call_logs))
        .route("/calls/{uid}/findings", get(call_findings))
        .route("/calls/{uid}/registrations", get(call_regs))
        .route("/calls/{uid}/related", get(call_related))
        .route("/calls/{uid}/export/{format}", get(call_export))
        .route("/saved-searches", get(saved_list).post(saved_create))
        .route("/saved-searches/{id}", delete(saved_delete))
        .route("/subscribers/{number}", get(subscriber))
        .route("/registrations/now", get(registered_now))
        .route("/registrations/failing", get(failing))
        .route("/live", get(live))
        .route("/queries/schema", get(schema))
        .route("/queries/library", get(library))
        .route("/queries/run", post(run_query))
        .route("/admin/audit", get(audit_log))
        .route("/admin/normalize-test", post(normalize_test))
        .route("/sources", get(sources_list))
        .route("/admin/sources/{ip}", put(source_alias))
        .route("/admin/{section}", get(admin_section))
        .fallback(|| async { not_found("No such API route") })
        .layer(middleware::from_fn(csrf));
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/readyz", get(ready))
        .nest("/api/v1", api)
        .fallback(static_file)
        .layer(middleware::from_fn(security_headers))
        .with_state(app)
}

async fn ready(State(a): S) -> Response {
    match sqlx::query("SELECT 1").execute(&a.db).await {
        Ok(_) => "ready".into_response(),
        Err(e) => {
            tracing::warn!("readiness check failed: {e}");
            problem(StatusCode::SERVICE_UNAVAILABLE, "Database unavailable", "MariaDB can't be reached.")
        }
    }
}

async fn static_file(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let (file, name) = match Assets::get(path) {
        Some(f) if !path.is_empty() => (f, path.to_string()),
        _ => match Assets::get("index.html") {
            Some(f) => (f, "index.html".to_string()),
            None => return (StatusCode::NOT_FOUND, "UI not built. Run: npm --prefix web run build").into_response(),
        },
    };
    let mime = mime_guess::from_path(&name).first_or_octet_stream();
    let cache = if name.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    ([(header::CONTENT_TYPE, mime.as_ref().to_string()), (header::CACHE_CONTROL, cache.to_string())], file.data.into_owned()).into_response()
}

async fn meta(State(a): S, _user: User) -> Json<Value> {
    let ch = a.storage.read().unwrap().clone();
    let (mut data_from, mut calls_total) = (Value::Null, json!(0));
    if let Some(ch) = &ch
        && let Ok(rows) = ch.select("SELECT toUnixTimestamp64Milli(min(start_ts)) AS f, uniq(call_uid) AS n FROM calls").await
        && let Some(r) = rows.first()
        && r["n"].as_u64().unwrap_or(0) > 0
    {
        data_from = r["f"].clone();
        calls_total = r["n"].clone();
    }
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "now": now_us() / 1000,
        "data_from": data_from,
        "calls_total": calls_total,
        "storage": ch.is_some(),
        "search_fields": search::FIELDS.iter().map(|(f, d)| json!({"field": f, "description": d})).collect::<Vec<_>>(),
        "facets": search::FACETS,
    }))
}


async fn catalogue(user: User) -> Response {
    if let Err(r) = user.need_any(&["dashboards_view", "explore"]) {
        return r;
    }
    Json(metrics::catalogue_json()).into_response()
}

fn query_failed(e: String) -> Response {
    problem(StatusCode::BAD_GATEWAY, "ClickHouse query failed", &e)
}

async fn metric_query(State(a): S, user: User, Json(q): Json<metrics::Query>) -> Response {
    if let Err(r) = user.need_any(&["dashboards_view", "explore"]) {
        return r;
    }
    let ch = match a.ch() {
        Ok(c) => c,
        Err(r) => return r,
    };
    let now = now_us();

    // A saved panel's SQL comes from the database, so anyone who can view the dashboard can run it.
    // SQL sent by the client needs its own permission.
    let mut sql_text = None;
    let (mut title, mut unit) = (q.title.clone().unwrap_or_default(), q.unit.clone().unwrap_or_default());
    if let Some(r) = &q.panel_ref {
        match dashboards::panel_sql(&a, &r.dashboard, r.panel).await {
            Ok(Some(p)) if p["query"]["mode"] == "sql" => {
                sql_text = p["query"]["sql"].as_str().map(str::to_string);
                title = p["title"].as_str().unwrap_or("").to_string();
                unit = p["options"]["unit"].as_str().unwrap_or("").to_string();
            }
            Ok(_) => return not_found("No such SQL panel"),
            Err(r) => return r,
        }
    } else if let Some(s) = &q.sql {
        if let Err(r) = user.need_any(&["dashboards_sql", "sql_workbench"]) {
            return r;
        }
        sql_text = Some(s.clone());
    }

    let opts = a.opts();
    if let Some(text) = sql_text {
        if !a.settings.bool("sql_enabled") {
            return problem(StatusCode::FORBIDDEN, "SQL is turned off", "SQL has been disabled in Settings.");
        }
        return match metrics::run_sql(&ch, &q, &text, &title, &unit, now, &opts).await {
            Ok(v) => Json(v).into_response(),
            Err(e) => problem(StatusCode::UNPROCESSABLE_ENTITY, "Query failed", &e),
        };
    }
    match metrics::run(&ch, now, &q, &opts).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => problem(StatusCode::BAD_REQUEST, "Bad metric query", &e),
    }
}

/// Distinct values of a filter field, for dropdowns.
async fn field_values(State(a): S, user: User, Path(field): Path<String>) -> Response {
    if let Err(r) = user.need_any(&["dashboards_view", "search_all", "explore"]) {
        return r;
    }
    if field == "capture" {
        let mut ids: HashMap<u32, Vec<String>> = HashMap::new();
        for x in a.ingest.agents.read().unwrap().values() {
            ids.entry(x.capture_id).or_default().push(x.label());
        }
        let mut list: Vec<(u32, Vec<String>)> = ids.into_iter().collect();
        list.sort();
        return Json(json!(list.iter().map(|(id, names)| json!({"value": id.to_string(), "label": format!("{id} · {}", names.join(", "))})).collect::<Vec<_>>())).into_response();
    }
    if field == "source" {
        let mut list: Vec<(String, String)> = a.ingest.agents.read().unwrap().values().map(|x| (x.ip.clone(), x.label())).collect();
        list.sort();
        return Json(json!(list.iter().map(|(ip, label)| json!({"value": ip, "label": if label == ip { ip.clone() } else { format!("{label} · {ip}") }})).collect::<Vec<_>>())).into_response();
    }
    let Some(expr) = search::facet_expr(&field) else { return Json(json!([])).into_response() };
    let ch = match a.ch() {
        Ok(c) => c,
        Err(_) => return Json(json!([])).into_response(),
    };
    let sql_text = format!("SELECT DISTINCT {expr} AS k FROM v_calls WHERE start_ts > {} HAVING k NOT IN ('', '0') ORDER BY k LIMIT 500", sql_literal_time(now_us() - DAY));
    match ch.select(&sql_text).await {
        Ok(rows) => Json(json!(rows.iter().map(|r| json!({"value": r["k"], "label": r["k"]})).collect::<Vec<_>>())).into_response(),
        Err(e) => query_failed(e),
    }
}


#[derive(Deserialize)]
struct SearchParams {
    q: Option<String>,
    from: Option<String>,
    to: Option<String>,
    tz: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
    points: Option<usize>,
    sort: Option<String>,
    dir: Option<String>,
    facets: Option<String>,
    columns: Option<String>,
    /// The source picked in the top bar.
    source: Option<String>,
}

/// The search conditions plus the source picked in the top bar.
fn with_source(tokens: &[search::Token], source: Option<&str>) -> String {
    let mut filter = search::to_sql(tokens);
    if let Some(ip) = source.filter(|s| !s.is_empty()) {
        let cond = format!("has(agent_ips, {})", sql::quote(ip));
        filter = if filter.is_empty() { cond } else { format!("{filter} AND {cond}") };
    }
    filter
}

fn mask_text(s: &str, keep: usize) -> String {
    let n = s.chars().count();
    if n <= keep {
        return s.to_string();
    }
    let tail: String = s.chars().skip(n - keep).collect();
    format!("{}{tail}", "•".repeat((n - keep).min(6)))
}

/// Text replacements that hide every party except `subject`. Long strings go first, so a raw
/// number that contains the normalized one is replaced whole.
fn mask_list(a: &App, user: &User, c: &Call, subject: Option<&str>) -> Vec<(String, String)> {
    if user.can("unmask_numbers") {
        return vec![];
    }
    let keep = a.settings.int("mask_keep_digits") as usize;
    let mut out = Vec::new();
    for (raw, n) in [(&c.caller, &c.caller_n), (&c.callee, &c.callee_n)] {
        if subject == Some(n.as_str()) {
            continue;
        }
        for text in [raw, n] {
            if text.len() >= 7 {
                out.push((text.clone(), mask_text(text, keep)));
            }
        }
    }
    out.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
    out
}

fn apply_mask(text: String, list: &[(String, String)]) -> String {
    list.iter().fold(text, |t, (from, to)| t.replace(from.as_str(), to))
}

fn json_text(text: String) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], text).into_response()
}

/// Without "Browse all calls" a search must name one number exactly (helpdesk use).
fn subject_of(tokens: &[search::Token]) -> Option<String> {
    tokens.iter().find(|t| matches!(t.field.as_str(), "number" | "caller" | "callee") && !t.value.contains('*') && t.op == ":" && !t.negated).map(|t| search::normalize(&t.value))
}

fn time_condition(from: i64, to: i64, filter: &str) -> String {
    let time = format!("start_ts >= {} AND start_ts < {}", sql_literal_time(from), sql_literal_time(to));
    if filter.is_empty() { time } else { format!("{time} AND {filter}") }
}

fn masked(call: &Call, masks: &[(String, String)]) -> Value {
    if masks.is_empty() { json!(call) } else { serde_json::from_str(&apply_mask(json!(call).to_string(), masks)).unwrap_or(Value::Null) }
}

async fn call_search(State(a): S, user: User, Query(p): Query<SearchParams>) -> Response {
    let q = p.q.clone().unwrap_or_default();
    let tokens = search::parse(&q);
    let subject = subject_of(&tokens);
    if !user.can("search_all") {
        if let Err(r) = user.need("subscribers_view") {
            return r;
        }
        if subject.is_none() {
            return problem(StatusCode::FORBIDDEN, "Enter a number", "Your account can only search for one number at a time.");
        }
    }
    let ch = match a.ch() {
        Ok(c) => c,
        Err(r) => return r,
    };
    let now = now_us();
    let to = p.to.as_deref().and_then(|x| parse_time(x, now)).unwrap_or(now).min(now);
    let default_from = parse_time(&a.settings.get("search_default_range"), now).unwrap_or(now - DAY);
    let mut from = p.from.as_deref().and_then(|x| parse_time(x, now)).unwrap_or(default_from);
    if let Some(l) = search::last_us(&tokens) {
        from = to - l;
    }
    let tz = parse_tz(p.tz.as_deref());
    let opts = a.opts();
    let cond = time_condition(from, to, &with_source(&tokens, p.source.as_deref()));

    let offset = p.offset.unwrap_or(0);
    let limit = p.limit.unwrap_or(a.settings.int("search_page_size") as usize).clamp(1, a.settings.int("search_max_page_size") as usize);
    let sort = p.sort.as_deref().filter(|s| sql::CALL_COLUMNS.iter().any(|c| c.0 == *s)).unwrap_or("start_ts");
    let dir = if p.dir.as_deref() == Some("asc") { "ASC" } else { "DESC" };
    let rows_sql = format!("SELECT {CALL_SELECT} FROM v_calls WHERE {cond} ORDER BY {sort} {dir} LIMIT {limit} OFFSET {offset}");
    let count_sql = format!("SELECT count() AS n FROM v_calls WHERE {cond}");

    // Time distribution, one series per status class, so the histogram explains the result.
    let step = metrics::pick_interval(from, to, None, Some(p.points.unwrap_or(120)), opts.max_points);
    let b = Buckets::new(from, to, step, tz);
    let classes = ["2xx", "3xx", "4xx", "5xx", "6xx", "in progress"];
    let hist_sql = format!("SELECT toUnixTimestamp(toStartOfInterval(start_ts, INTERVAL {step} SECOND, '{tz}')) AS t, if(final_status = 0, 'in progress', concat(toString(intDiv(final_status, 100)), 'xx')) AS k, count() AS c FROM v_calls WHERE {cond} GROUP BY t, k");

    let facet_size = a.settings.int("search_facet_size");
    let requested: Vec<&str> = p.facets.as_deref().map(|f| f.split(',').filter(|x| search::FACETS.contains(x)).collect()).unwrap_or_else(|| search::FACETS.to_vec());
    let facet_sql: Vec<String> = requested.iter().filter_map(|name| search::facet_expr(name)).map(|expr| format!("SELECT {expr} AS k, count() AS c FROM v_calls WHERE {cond} GROUP BY k HAVING k NOT IN ('', '0') ORDER BY c DESC, k LIMIT {facet_size}")).collect();

    let (rows, count, hist, facets) = tokio::join!(ch.select(&rows_sql), ch.select(&count_sql), ch.select(&hist_sql), join_all(facet_sql.iter().map(|s| ch.select(s))));
    let (rows, count, hist) = match (rows, count, hist) {
        (Ok(r), Ok(c), Ok(h)) => (r, c, h),
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => return query_failed(e),
    };
    let total = count.first().and_then(|r| r["n"].as_u64()).unwrap_or(0);

    let mut series = vec![vec![0u64; b.n]; classes.len()];
    for r in &hist {
        if let (Some(i), Some(k)) = (r["t"].as_i64().and_then(|t| b.index(t * SEC)), classes.iter().position(|c| Some(*c) == r["k"].as_str())) {
            series[k][i] += r["c"].as_u64().unwrap_or(0);
        }
    }

    let (captures, sources): (HashMap<u32, String>, HashMap<String, String>) = {
        let agents = a.ingest.agents.read().unwrap();
        (agents.values().map(|x| (x.capture_id, x.label())).collect(), agents.values().map(|x| (x.ip.clone(), x.label())).collect())
    };
    let mut facet_json = serde_json::Map::new();
    for (name, result) in requested.iter().zip(facets) {
        let list: Vec<Value> = result.unwrap_or_default().iter().map(|r| {
            let key = r["k"].as_str().unwrap_or("");
            let label = match *name {
                "capture" => key.parse().ok().and_then(|id: u32| captures.get(&id)).map(|n| format!("{key} · {n}")),
                "source" => sources.get(key).filter(|n| *n != key).cloned(),
                _ => None,
            };
            json!({"value": key, "count": r["c"], "label": label.unwrap_or_else(|| key.to_string())})
        }).collect();
        facet_json.insert(name.to_string(), json!(list));
    }

    let calls: Vec<Value> = rows.iter().map(Call::from_row).map(|c| masked(&c, &mask_list(&a, &user, &c, subject.as_deref()))).collect();

    let mut subscribers = Vec::new();
    for t in tokens.iter().filter(|t| matches!(t.field.as_str(), "number" | "caller" | "callee") && !t.value.contains('*')) {
        let n = search::normalize(&t.value);
        if let Ok(events) = reg::events_for(&ch, &n, now - 2 * DAY, now).await
            && let Some(state) = reg::state_of(&n, &events, now)
        {
            subscribers.push(state);
        }
    }

    let hints = if total == 0 { zero_result_hints(&a, &tokens) } else { vec![] };

    Json(json!({
        "tokens": tokens,
        "total": total,
        "offset": offset,
        "limit": limit,
        "rows": calls,
        "from": from / 1000,
        "to": to / 1000,
        "facets": facet_json,
        "histogram": {
            "buckets": b.starts_ms(),
            "interval_s": step,
            "series": classes.iter().zip(series.iter()).map(|(n, v)| json!({"name": n, "values": v})).collect::<Vec<_>>(),
        },
        "subscribers": subscribers,
        "hints": hints,
    })).into_response()
}

async fn export_csv(State(a): S, user: User, Query(p): Query<SearchParams>) -> Response {
    if let Err(r) = user.need("export_bulk").and_then(|_| user.need("search_all")) {
        return r;
    }
    let ch = match a.ch() {
        Ok(c) => c,
        Err(r) => return r,
    };
    let now = now_us();
    let tokens = search::parse(p.q.as_deref().unwrap_or(""));
    let to = p.to.as_deref().and_then(|x| parse_time(x, now)).unwrap_or(now).min(now);
    let mut from = p.from.as_deref().and_then(|x| parse_time(x, now)).unwrap_or(now - DAY);
    if let Some(l) = search::last_us(&tokens) {
        from = to - l;
    }
    let default_columns = "start_ts,caller_n,callee_n,direction,trunk,final_status,pdd_ms,talk_ms,min_mos,findings";
    let columns: Vec<&str> = p.columns.as_deref().unwrap_or(default_columns).split(',').filter(|c| sql::CALL_COLUMNS.iter().any(|d| d.0 == *c)).collect();
    let cond = time_condition(from, to, &with_source(&tokens, p.source.as_deref()));
    let rows = match ch.select(&format!("SELECT {CALL_SELECT} FROM v_calls WHERE {cond} ORDER BY start_ts DESC LIMIT {}", a.settings.int("export_max_rows"))).await {
        Ok(r) => r,
        Err(e) => return query_failed(e),
    };
    let cell = |v: &Value| -> String {
        let text = match v {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            Value::Array(list) => list.iter().map(|x| x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string())).collect::<Vec<_>>().join("|"),
            other => other.to_string(),
        };
        // Captured text can start with = + - or @, which a spreadsheet would run as a formula.
        let text = if matches!(v, Value::String(_) | Value::Array(_)) && text.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{text}") } else { text };
        if text.contains([',', '"', '\n', '\r']) { format!("\"{}\"", text.replace('"', "\"\"")) } else { text }
    };
    let mut body = columns.join(",");
    body.push('\n');
    for r in &rows {
        let c = Call::from_row(r);
        let row = json!(c);
        let line = columns.iter().map(|col| cell(&row[*col])).collect::<Vec<_>>().join(",");
        body.push_str(&apply_mask(line, &mask_list(&a, &user, &c, None)));
        body.push('\n');
    }
    db::audit(&a.db, &user.username, &user.ip, "export.csv", p.q.as_deref().unwrap_or(""), "success", "info", "");
    ([(header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()), (header::CONTENT_DISPOSITION, "attachment; filename=\"calls.csv\"".to_string())], body).into_response()
}

fn zero_result_hints(a: &App, tokens: &[search::Token]) -> Vec<String> {
    let mut out = Vec::new();
    for t in tokens {
        if let Some(d) = &t.detected {
            out.push(format!("Searched \"{}\" as {d}.", t.raw));
        }
        if t.field == "number" && !t.value.contains('*') {
            let tail: String = t.value.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
            out.push(format!("Try a suffix search: *{tail}"));
        }
    }
    if a.ingest.agents.read().unwrap().is_empty() {
        out.push("No capture agent has sent HEP to this panel yet.".into());
    } else {
        out.push("Try a wider time range.".into());
    }
    out
}


#[derive(Deserialize)]
struct Subject {
    subject: Option<String>,
}

struct Loaded {
    ch: Ch,
    call: Call,
    masks: Vec<(String, String)>,
}

/// Loads a call after checking that this user may open it.
async fn load_call(a: &App, user: &User, uid: &str, subject: Option<&str>) -> Result<Loaded, Response> {
    user.need_any(&["search_all", "subscribers_view"])?;
    let ch = a.ch()?;
    uuid::Uuid::parse_str(uid).map_err(|_| problem(StatusCode::BAD_REQUEST, "Bad call id", uid))?;
    let rows = ch.select(&format!("SELECT {CALL_SELECT} FROM v_calls WHERE call_uid = {} LIMIT 1", sql::quote(uid))).await.map_err(query_failed)?;
    let call = Call::from_row(rows.first().ok_or_else(|| not_found("No such call"))?);
    let subject = subject.map(search::normalize);
    if !user.can("search_all") && !subject.as_deref().is_some_and(|n| n == call.caller_n || n == call.callee_n) {
        return Err(problem(StatusCode::FORBIDDEN, "Enter a number", "Your account can only open calls of the number you looked up."));
    }
    let masks = mask_list(a, user, &call, subject.as_deref());
    Ok(Loaded { ch, call, masks })
}

macro_rules! call_route {
    ($name:ident, |$l:ident, $user:ident| $body:block) => {
        async fn $name(State(a): S, $user: User, Path(uid): Path<String>, Query(p): Query<Subject>) -> Response {
            let $l = match load_call(&a, &$user, &uid, p.subject.as_deref()).await {
                Ok(l) => l,
                Err(r) => return r,
            };
            let value: Result<Value, String> = async { $body }.await;
            match value {
                Ok(v) if $l.masks.is_empty() => Json(v).into_response(),
                Ok(v) => json_text(apply_mask(v.to_string(), &$l.masks)),
                Err(e) => query_failed(e),
            }
        }
    };
}

async fn visible_flow(user: &User, l: &Loaded) -> Result<flow::Flow, String> {
    let mut f = flow::load(&l.ch, &l.call).await?;
    if !user.can("view_payload") {
        for m in &mut f.messages {
            m.headers.clear();
            m.raw.clear();
            m.sdp = None;
            m.diff.clear();
        }
    }
    Ok(f)
}

call_route!(call_get, |l, user| {
    let f = visible_flow(&user, &l).await?;
    let mut call = l.call.clone();
    call.msg_count = f.messages.len() as u32;
    Ok(json!({"call": call, "columns": f.columns}))
});
call_route!(call_flow, |l, user| { Ok(json!(visible_flow(&user, &l).await?)) });
call_route!(call_media, |l, user| {
    let f = visible_flow(&user, &l).await?;
    let sdp: Vec<Value> = f.messages.iter().filter(|m| m.sdp.is_some() && m.retx == 0).map(|m| json!({
        "idx": m.idx, "leg": m.leg, "label": m.label, "from": f.columns[m.src].name, "to": f.columns[m.dst].name, "sdp": m.sdp,
    })).collect();
    Ok(json!({"streams": callview::streams(&l.ch, &l.call).await?, "sdp": sdp, "media_node": l.call.media_node}))
});
call_route!(call_logs, |l, user| {
    let _ = &user;
    callview::logs(&l.ch, &l.call).await
});
call_route!(call_findings, |l, user| {
    let f = visible_flow(&user, &l).await?;
    Ok(callview::findings(&l.call, &f))
});
call_route!(call_regs, |l, user| {
    let _ = &user;
    let start = l.call.start_ts.map(|t| t.0).unwrap_or(0);
    let mut parties = Vec::new();
    for (role, n) in [("Caller", &l.call.caller_n), ("Callee", &l.call.callee_n)] {
        let events = reg::events_for(&l.ch, n, start - DAY, start + HOUR).await?;
        let before: Vec<reg::RegEvent> = events.iter().filter(|e| e.ts.0 <= start).cloned().collect();
        if let Some(state) = reg::state_of(n, &before, start) {
            let nearby: Vec<&reg::RegEvent> = events.iter().filter(|e| e.ts.0 >= start - HOUR).collect();
            parties.push(json!({"role": role, "number": n, "state": state, "events": nearby}));
        }
    }
    Ok(json!(parties))
});
call_route!(call_related, |l, user| {
    let _ = &user;
    let start = l.call.start_ts.map(|t| t.0).unwrap_or(0);
    let cond = time_condition(start - HOUR, start + HOUR, &format!("caller_n = {} AND callee_n = {} AND call_uid != {}", sql::quote(&l.call.caller_n), sql::quote(&l.call.callee_n), sql::quote(&l.call.call_uid)));
    let rows = l.ch.select(&format!("SELECT {CALL_SELECT} FROM v_calls WHERE {cond} ORDER BY start_ts LIMIT 30")).await?;
    Ok(json!(rows.iter().map(Call::from_row).map(|o| {
        let retry = o.start_ts.is_some_and(|t| (t.0 - start).abs() <= 2 * MIN);
        json!({"relation": if retry { "retry" } else { "same parties" }, "call": o})
    }).collect::<Vec<_>>()))
});

async fn by_call_id(State(a): S, user: User, Path(id): Path<String>) -> Response {
    if let Err(r) = user.need("search_all") {
        return r;
    }
    let ch = match a.ch() {
        Ok(c) => c,
        Err(r) => return r,
    };
    match ch.select(&format!("SELECT call_uid FROM v_calls WHERE has(legs, {}) ORDER BY start_ts DESC LIMIT 1", sql::quote(&id))).await {
        Ok(rows) => match rows.first() {
            Some(r) => Json(json!({"call_uid": r["call_uid"]})).into_response(),
            None => not_found("No call with this Call-ID"),
        },
        Err(e) => query_failed(e),
    }
}

async fn call_export(State(a): S, user: User, Path((uid, format)): Path<(String, String)>, Query(p): Query<Subject>) -> Response {
    if let Err(r) = user.need("export_single").and_then(|_| user.need("view_payload")) {
        return r;
    }
    let l = match load_call(&a, &user, &uid, p.subject.as_deref()).await {
        Ok(l) => l,
        Err(r) => {
            db::audit(&a.db, &user.username, &user.ip, "export.call", &uid, "denied", "info", &format);
            return r;
        }
    };
    let mut f = match flow::load(&l.ch, &l.call).await {
        Ok(f) => f,
        Err(e) => return query_failed(e),
    };
    for m in &mut f.messages {
        m.raw = apply_mask(std::mem::take(&mut m.raw), &l.masks);
    }
    let stem = format!("call-{}", l.call.call_uid.chars().take(8).collect::<String>());
    let (name, mime, body) = match format.as_str() {
        "text" => (format!("{stem}.txt"), "text/plain; charset=utf-8", apply_mask(callview::text_export(&l.call, &f), &l.masks).into_bytes()),
        "pcap" => (format!("{stem}.pcap"), "application/vnd.tcpdump.pcap", callview::pcap_export(&f)),
        _ => {
            let streams = match callview::streams(&l.ch, &l.call).await {
                Ok(s) => s,
                Err(e) => return query_failed(e),
            };
            let body = json!({"call": l.call, "flow": f, "streams": streams, "findings": callview::findings(&l.call, &f)});
            (format!("{stem}.json"), "application/json", apply_mask(serde_json::to_string_pretty(&body).unwrap_or_default(), &l.masks).into_bytes())
        }
    };
    db::audit(&a.db, &user.username, &user.ip, "export.call", &uid, "success", "info", &format);
    ([
        (header::CONTENT_TYPE, mime.to_string()),
        (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
    ], body).into_response()
}


#[derive(Deserialize)]
struct SavedBody {
    name: String,
    q: String,
    #[serde(default)]
    shared: bool,
}

async fn saved_list(State(a): S, user: User) -> Response {
    match sqlx::query("SELECT s.id, s.name, s.q, s.shared, s.owner_id, u.username FROM saved_searches s JOIN users u ON u.id = s.owner_id WHERE s.owner_id = ? OR s.shared = 1 ORDER BY s.name").bind(user.id).fetch_all(&a.db).await {
        Ok(rows) => Json(json!(rows.iter().map(|r| json!({
            "id": r.get::<i64, _>("id"), "name": r.get::<String, _>("name"), "q": r.get::<String, _>("q"), "shared": r.get::<i8, _>("shared") != 0,
            "owner": r.get::<String, _>("username"), "mine": r.get::<i64, _>("owner_id") == user.id,
        })).collect::<Vec<_>>())).into_response(),
        Err(e) => db_error(e),
    }
}

async fn saved_create(State(a): S, user: User, Json(b): Json<SavedBody>) -> Response {
    if let Err(r) = user.need("search_all") {
        return r;
    }
    if b.name.trim().is_empty() || b.name.trim().chars().count() > 128 || b.q.len() > 4096 {
        return problem(StatusCode::BAD_REQUEST, "Invalid search", "Give the search a name of up to 128 characters.");
    }
    match sqlx::query("INSERT INTO saved_searches (owner_id, name, q, shared, created_at) VALUES (?, ?, ?, ?, ?)").bind(user.id).bind(b.name.trim()).bind(&b.q).bind(b.shared as i8).bind(utc_now()).execute(&a.db).await {
        Ok(r) => Json(json!({"id": r.last_insert_id()})).into_response(),
        Err(e) => db_error(e),
    }
}

async fn saved_delete(State(a): S, user: User, Path(id): Path<i64>) -> Response {
    match sqlx::query("DELETE FROM saved_searches WHERE id = ? AND owner_id = ?").bind(id).bind(user.id).execute(&a.db).await {
        Ok(_) => Json(json!({"ok": true})).into_response(),
        Err(e) => db_error(e),
    }
}

async fn subscriber(State(a): S, user: User, Path(number): Path<String>, Query(p): Query<SearchParams>) -> Response {
    if let Err(r) = user.need("subscribers_view") {
        return r;
    }
    let ch = match a.ch() {
        Ok(c) => c,
        Err(r) => return r,
    };
    let n = search::normalize(&number);
    db::audit(&a.db, &user.username, &user.ip, "subscriber.view", &n, "success", "info", "");
    let now = now_us();
    let from = p.from.as_deref().and_then(|x| parse_time(x, now)).unwrap_or(now - DAY).max(now - 2 * DAY);
    let cond = time_condition(now - 7 * DAY, now + MIN, &format!("(caller_n = {0} OR callee_n = {0})", sql::quote(&n)));
    let calls_sql = format!("SELECT {CALL_SELECT} FROM v_calls WHERE {cond} ORDER BY start_ts DESC LIMIT 1000");
    let (events, rows) = tokio::join!(reg::events_for(&ch, &n, now - 2 * DAY, now + MIN), ch.select(&calls_sql));
    let (all_events, rows) = match (events, rows) {
        (Ok(e), Ok(r)) => (e, r),
        (Err(e), _) | (_, Err(e)) => return query_failed(e),
    };
    let window: Vec<Call> = rows.iter().map(Call::from_row).collect();
    if all_events.is_empty() && window.is_empty() {
        return problem(StatusCode::NOT_FOUND, "Not seen", &format!("Nothing was captured for number {n} (from \"{number}\") in the last 7 days."));
    }
    let state = reg::state_of(&n, &all_events, now).unwrap_or_else(|| reg::unseen(&n));
    let events: Vec<reg::RegEvent> = all_events.into_iter().filter(|e| e.ts.0 >= from).collect();
    let timeline = reg::timeline(&events, from, now);

    let calls: Vec<Value> = window.iter().take(100).map(|c| masked(c, &mask_list(&a, &user, c, Some(&n)))).collect();

    let recent: Vec<&Call> = window.iter().filter(|c| c.end_ts.is_some()).take(10).collect();
    let mut summary: Vec<Value> = Vec::new();
    if state.status == "failing"
        && let (Some(f), Some(since)) = (&state.last_failure, state.failing_since)
    {
        summary.push(json!({"severity": "critical", "text": format!("Registration failing with {}", f.code), "since": since}));
    }
    for (code, label) in [("POOR_QUALITY", "poor quality"), ("AUTH_LOOP", "authentication loop"), ("ACK_MISSING", "missing ACK"), ("DROP_AFTER_32S", "drop after 32 s")] {
        let hits = recent.iter().filter(|c| c.findings.iter().any(|f| f == code)).count();
        if hits > 0 {
            summary.push(json!({"severity": callview::severity_of(code), "text": format!("{label} in {hits} of the last {} calls", recent.len())}));
        }
    }
    let distinct_ips = events.iter().map(|e| e.src_ip.as_str()).collect::<std::collections::HashSet<_>>().len();
    if distinct_ips > 4 {
        summary.push(json!({"severity": "warning", "text": format!("Registered from {distinct_ips} different addresses in this period")}));
    }

    let mut daily: Vec<Value> = Vec::new();
    for d in (0..7).rev() {
        let (a0, a1) = (now - (d + 1) * DAY, now - d * DAY);
        let day: Vec<&Call> = window.iter().filter(|c| c.end_ts.is_some() && c.start_ts.is_some_and(|t| t.0 >= a0 && t.0 < a1)).collect();
        let mos: Vec<f32> = day.iter().filter_map(|c| c.min_mos).collect();
        let loss: Vec<f32> = day.iter().filter_map(|c| c.max_loss_pct).collect();
        daily.push(json!({
            "day": a1 / 1000,
            "mos": (!mos.is_empty()).then(|| mos.iter().sum::<f32>() / mos.len() as f32),
            "loss": (!loss.is_empty()).then(|| loss.iter().sum::<f32>() / loss.len() as f32),
            "calls": day.len(),
        }));
    }

    Json(json!({
        "number": n,
        "input": number,
        "state": state,
        "events": events.iter().rev().take(200).collect::<Vec<_>>(),
        "timeline": timeline,
        "calls": calls,
        "calls_7d": window.len(),
        "summary": summary,
        "daily": daily,
        "from": from / 1000,
        "to": now / 1000,
    })).into_response()
}

#[derive(Deserialize)]
struct RegParams {
    status: Option<String>,
    q: Option<String>,
    limit: Option<usize>,
}

async fn registered_now(State(a): S, user: User, Query(p): Query<RegParams>) -> Response {
    if let Err(r) = user.need("search_all") {
        return r;
    }
    let ch = match a.ch() {
        Ok(c) => c,
        Err(r) => return r,
    };
    let all = match reg::all_states(&ch, now_us()).await {
        Ok(v) => v,
        Err(e) => return query_failed(e),
    };
    let tally = |f: &dyn Fn(&reg::RegState) -> &str| -> Value {
        let mut m: HashMap<String, u32> = HashMap::new();
        for r in &all {
            *m.entry(f(r).to_string()).or_default() += 1;
        }
        let mut v: Vec<_> = m.into_iter().filter(|(k, _)| !k.is_empty()).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        json!(v.iter().map(|(k, c)| json!({"value": k, "count": c})).collect::<Vec<_>>())
    };
    let facets = json!({"status": tally(&|r| r.status), "group": tally(&|r| &r.group), "vendor": tally(&|r| &r.ua_vendor)});
    let q = p.q.unwrap_or_default().to_lowercase();
    let want = p.status.unwrap_or_default();
    let matched: Vec<&reg::RegState> = all.iter()
        .filter(|r| want.is_empty() || r.status == want)
        .filter(|r| q.is_empty() || r.number.contains(&search::normalize(&q)) && !search::normalize(&q).is_empty() || r.src_ip.contains(&q) || r.ua_model.to_lowercase().contains(&q))
        .collect();
    let limit = p.limit.unwrap_or(150).min(500);
    Json(json!({"total": matched.len(), "rows": matched.iter().take(limit).collect::<Vec<_>>(), "facets": facets})).into_response()
}

async fn failing(State(a): S, user: User) -> Response {
    if let Err(r) = user.need("dashboards_view") {
        return r;
    }
    let ch = match a.ch() {
        Ok(c) => c,
        Err(r) => return r,
    };
    match reg::all_states(&ch, now_us()).await {
        Ok(all) => {
            let mut rows: Vec<reg::RegState> = all.into_iter().filter(|s| s.status == "failing").collect();
            rows.sort_by_key(|s| std::cmp::Reverse(s.consecutive_failures));
            rows.truncate(12);
            Json(json!(rows)).into_response()
        }
        Err(e) => query_failed(e),
    }
}


/// Releases a live-capture slot when the stream ends, however it ends.
struct LiveSlot(Arc<App>);

impl Drop for LiveSlot {
    fn drop(&mut self) {
        self.0.live_sessions.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn live(State(a): S, user: User) -> Response {
    if let Err(r) = user.need("live_view") {
        return r;
    }
    if !a.settings.bool("live_enabled") {
        return problem(StatusCode::FORBIDDEN, "Live capture is off", "Turn it on in Settings first.");
    }
    let max = a.settings.int("live_max_sessions") as usize;
    if a.live_sessions.fetch_add(1, Ordering::SeqCst) >= max {
        a.live_sessions.fetch_sub(1, Ordering::SeqCst);
        return problem(StatusCode::TOO_MANY_REQUESTS, "Too many live streams", &format!("The limit is {max} at a time."));
    }
    db::audit(&a.db, &user.username, &user.ip, "live.start", "", "success", "info", "");
    let deadline = Instant::now() + Duration::from_secs(a.settings.int("live_idle_minutes") as u64 * 60);
    let tick = Duration::from_millis(a.settings.int("live_tick_ms") as u64);
    let keep = (!user.can("unmask_numbers")).then(|| a.settings.int("mask_keep_digits") as usize);
    let rx = a.ingest.live.subscribe();
    let slot = LiveSlot(a.clone());
    let stream = futures_util::stream::unfold((rx, slot), move |(mut rx, slot)| async move {
        if Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(tick).await;
        let mut out: Vec<String> = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(event) if out.len() < 2000 => out.push(match keep {
                    Some(k) => mask_live(&event, k),
                    None => event.to_string(),
                }),
                Ok(_) | Err(TryRecvError::Lagged(_)) => {}
                Err(_) => break,
            }
        }
        Some((Ok::<Event, Infallible>(Event::default().data(format!("[{}]", out.join(",")))), (rx, slot)))
    });
    Sse::new(stream).keep_alive(KeepAlive::default()).into_response()
}

fn mask_live(event: &str, keep: usize) -> String {
    let Ok(mut v) = serde_json::from_str::<Value>(event) else { return String::from("null") };
    for key in ["caller", "callee"] {
        if let Some(s) = v[key].as_str() {
            v[key] = json!(mask_text(s, keep));
        }
    }
    v.to_string()
}


fn need_sql(a: &App, user: &User) -> Result<(), Response> {
    user.need("sql_workbench")?;
    if a.settings.bool("sql_enabled") {
        Ok(())
    } else {
        Err(problem(StatusCode::FORBIDDEN, "SQL is turned off", "SQL has been disabled in Settings."))
    }
}

async fn schema(State(a): S, user: User) -> Response {
    if let Err(r) = need_sql(&a, &user) {
        return r;
    }
    Json(sql::schema_json()).into_response()
}

async fn library(State(a): S, user: User) -> Response {
    if let Err(r) = need_sql(&a, &user) {
        return r;
    }
    Json(sql::library()).into_response()
}

#[derive(Deserialize)]
struct SqlBody {
    sql: String,
    from: Option<String>,
    to: Option<String>,
    tz: Option<String>,
}

async fn run_query(State(a): S, user: User, Json(b): Json<SqlBody>) -> Response {
    if let Err(r) = need_sql(&a, &user) {
        return r;
    }
    let ch = match a.ch() {
        Ok(c) => c,
        Err(r) => return r,
    };
    let now = now_us();
    let from = b.from.as_deref().and_then(|x| parse_time(x, now)).unwrap_or(now - DAY);
    let to = b.to.as_deref().and_then(|x| parse_time(x, now)).unwrap_or(now);
    let tz = parse_tz(b.tz.as_deref());
    let opts = a.opts();
    let step = metrics::pick_interval(from, to, None, Some(300), opts.max_points);
    let expanded = sql::expand_macros(&b.sql, from / 1000, to / 1000, step, tz, "", &HashMap::new());
    let started = Instant::now();
    let hash = hex::encode(Sha256::digest(b.sql.as_bytes()));
    match ch.query_readonly(&expanded, opts.timeout_s, opts.max_rows).await {
        Ok(t) => {
            db::audit(&a.db, &user.username, &user.ip, "sql.query", &hash[..16], "success", "info", &format!("{} rows", t.rows.len()));
            Json(json!({
                "columns": t.columns.iter().map(|c| json!({"name": c.name, "type": c.ty})).collect::<Vec<_>>(),
                "rows": t.rows,
                "stats": {"rows_read": t.rows_read, "duration_ms": started.elapsed().as_millis() as u64, "truncated": t.truncated},
            })).into_response()
        }
        Err(e) => {
            db::audit(&a.db, &user.username, &user.ip, "sql.query", &hash[..16], "failure", "info", &e);
            problem(StatusCode::UNPROCESSABLE_ENTITY, "Query failed", &e)
        }
    }
}

fn source_status(x: &crate::ingest::AgentStats, now: i64) -> &'static str {
    match x.last_seen.map(|t| now - t.0) {
        Some(i) if i < 2 * MIN && x.clock_offset_ms.abs() > 1000.0 => "clock skew",
        Some(i) if i < 2 * MIN => "active",
        _ => "silent",
    }
}

fn agents_json(a: &App, now: i64) -> Value {
    let mut rows: Vec<crate::ingest::AgentStats> = a.ingest.agents.read().unwrap().values().cloned().collect();
    rows.sort_by(|x, y| x.ip.cmp(&y.ip));
    json!(rows.iter().map(|x| json!({
        "ip": x.ip, "alias": x.alias, "label": x.label(), "capture_id": x.capture_id, "hep_version": x.hep_version, "packets": x.packets, "bytes": x.bytes,
        "errors": x.errors, "offset_ms": x.clock_offset_ms.round(), "last_seen": x.last_seen, "status": source_status(x, now),
    })).collect::<Vec<_>>())
}

/// The HEP senders, for the source picker in the top bar.
async fn sources_list(State(a): S, _user: User) -> Json<Value> {
    let now = now_us();
    let mut rows: Vec<crate::ingest::AgentStats> = a.ingest.agents.read().unwrap().values().cloned().collect();
    rows.sort_by(|x, y| x.label().to_lowercase().cmp(&y.label().to_lowercase()));
    Json(json!(rows.iter().map(|x| json!({"ip": x.ip, "alias": x.alias, "label": x.label(), "capture_id": x.capture_id, "status": source_status(x, now)})).collect::<Vec<_>>()))
}

async fn system_json(a: &App) -> Value {
    let ch = a.storage.read().unwrap().clone();
    let mut clickhouse = json!(null);
    if let Some(ch) = ch {
        let version = ch.version().await;
        let tables = ch.select(&format!("SELECT table, sum(rows) AS rows, sum(bytes_on_disk) AS bytes, count() AS parts FROM system.parts WHERE active AND database = {} GROUP BY table ORDER BY bytes DESC", sql::quote(&ch.database))).await;
        clickhouse = json!({"url": ch.url, "database": ch.database, "connected": version.is_ok(), "version": version.unwrap_or_else(|e| e), "tables": tables.unwrap_or_default()});
    }
    let mariadb: String = sqlx::query_scalar("SELECT VERSION()").fetch_one(&a.db).await.unwrap_or_default();
    let mut drops: Vec<(String, u64)> = a.ingest.totals.lock().unwrap().iter().map(|(k, v)| (k.clone(), *v)).collect();
    drops.sort_by_key(|d| std::cmp::Reverse(d.1));
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "listeners": {"hep_udp": a.boot.hep_udp, "hep_tcp": a.boot.hep_tcp},
        "auth_key": !a.settings.get("hep_auth_key").is_empty(),
        "clickhouse": clickhouse,
        "mariadb": mariadb,
        "drops": drops.iter().map(|(k, v)| json!({"reason": k, "count": v})).collect::<Vec<_>>(),
        "open_dialogs": a.ingest.dirty.lock().unwrap().len(),
    })
}

async fn admin_section(State(a): S, user: User, Path(section): Path<String>) -> Response {
    if let Err(r) = user.need("system_view") {
        return r;
    }
    match section.as_str() {
        "sources" | "agents" => Json(agents_json(&a, now_us())).into_response(),
        "system" => Json(system_json(&a).await).into_response(),
        _ => not_found("No such section"),
    }
}

#[derive(Deserialize)]
struct AliasBody {
    alias: String,
}

/// Names a source. An empty alias goes back to showing the address.
async fn source_alias(State(a): S, user: User, Path(ip): Path<String>, Json(b): Json<AliasBody>) -> Response {
    if let Err(r) = user.need("settings_manage") {
        return r;
    }
    let alias = b.alias.trim();
    if alias.chars().count() > 96 {
        return problem(StatusCode::BAD_REQUEST, "Too long", "Use up to 96 characters.");
    }
    if !a.ingest.agents.read().unwrap().contains_key(&ip) {
        return not_found("No source with this address");
    }
    match sqlx::query("INSERT INTO sources (ip, alias) VALUES (?, ?) ON DUPLICATE KEY UPDATE alias = VALUES(alias)").bind(&ip).bind(alias).execute(&a.db).await {
        Ok(_) => {
            if let Some(source) = a.ingest.agents.write().unwrap().get_mut(&ip) {
                source.alias = alias.to_string();
            }
            db::audit(&a.db, &user.username, &user.ip, "source.alias", &ip, "success", "notice", alias);
            Json(json!({"ok": true})).into_response()
        }
        Err(e) => db_error(e),
    }
}

async fn audit_log(State(a): S, user: User) -> Response {
    if let Err(r) = user.need("audit_read") {
        return r;
    }
    match sqlx::query("SELECT id, ts, actor, src_ip, action, object, outcome, severity, details FROM audit_events ORDER BY id DESC LIMIT 500").fetch_all(&a.db).await {
        Ok(rows) => Json(json!(rows.iter().map(|r| json!({
            "id": r.get::<i64, _>("id"), "ts": iso(r.get("ts")), "actor": r.get::<String, _>("actor"), "src_ip": r.get::<String, _>("src_ip"),
            "action": r.get::<String, _>("action"), "object": r.get::<String, _>("object"), "outcome": r.get::<String, _>("outcome"),
            "severity": r.get::<String, _>("severity"), "details": r.get::<String, _>("details"),
        })).collect::<Vec<_>>())).into_response(),
        Err(e) => db_error(e),
    }
}

#[derive(Deserialize)]
struct NormBody {
    number: String,
}

async fn normalize_test(user: User, Json(b): Json<NormBody>) -> Response {
    if let Err(r) = user.need("system_view") {
        return r;
    }
    let n = search::normalize(&b.number);
    let rev: String = n.chars().rev().collect();
    Json(json!({"input": b.number, "normalized": n, "reversed": rev})).into_response()
}