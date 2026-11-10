//! Configuration. Every application setting has an env var, `VP_<KEY>`. A value set in the
//! environment wins and is shown as locked in the UI; otherwise the value saved in MariaDB is used,
//! then the default. Only the database connection is required.

use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::RwLock;

#[derive(Clone, Copy)]
pub enum Kind {
    Str,
    Int(i64, i64),
    Bool,
    Choice(&'static [&'static str]),
}

pub struct Def {
    pub key: &'static str,
    pub kind: Kind,
    pub default: &'static str,
    pub group: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    /// Takes effect only after a restart.
    pub restart: bool,
    /// Sent to every signed-in browser (and to the login page when `login_visible`).
    pub public: bool,
}

const fn d(key: &'static str, kind: Kind, default: &'static str, group: &'static str, label: &'static str, help: &'static str, restart: bool, public: bool) -> Def {
    Def { key, kind, default, group, label, help, restart, public }
}

use Kind::*;

pub const DEFS: &[Def] = &[
    d("app_name", Str, "voip-panel", "Branding", "Application name", "Shown in the sidebar, the login page and the browser tab.", false, true),
    d("accent_color", Str, "", "Branding", "Accent color", "CSS color for buttons and highlights, e.g. #2a78d6. Empty keeps the default.", false, true),
    d("default_theme", Choice(&["system", "light", "dark"]), "system", "Branding", "Default theme", "Used until a user picks their own.", false, true),
    d("login_message", Str, "", "Branding", "Login message", "Text shown under the sign-in form.", false, true),
    d("banner_text", Str, "", "Branding", "Banner", "One line shown at the top of every page, e.g. Staging system.", false, true),

    d("default_tz", Str, "Asia/Baghdad", "Time", "Default time zone", "IANA name. Users can override it in their preferences.", false, true),
    d("time_zones", Str, "Asia/Baghdad,UTC,Europe/London,Asia/Dubai", "Time", "Time zones offered", "Comma-separated list for the time picker.", false, true),

    d("home_dashboard", Str, "overview", "Dashboards", "Home dashboard", "UID of the dashboard shown on the Home page.", false, true),
    d("default_range", Str, "now-6h", "Dashboards", "Default time range", "Used when a dashboard or URL does not set one, e.g. now-1h.", false, true),
    d("default_refresh_seconds", Int(0, 86400), "30", "Dashboards", "Default auto refresh (s)", "0 turns it off.", false, true),
    d("shared_crosshair", Bool, "false", "Dashboards", "Link charts on hover", "When on, pointing at one chart shows the crosshair on every chart of the dashboard. Users and dashboards can override it.", false, true),
    d("chart_max_points", Int(50, 5000), "1500", "Dashboards", "Max points per chart", "Upper limit for automatic bucket sizes.", false, false),
    d("chart_top_series", Int(1, 8), "8", "Dashboards", "Max series per chart", "Extra values fold into \"other\". Colors stay distinct up to 8.", false, false),
    d("settle_window_seconds", Int(0, 3600), "60", "Dashboards", "Settle window (s)", "Newer buckets are drawn dashed or hatched.", false, false),
    d("dashboard_versions_keep", Int(1, 1000), "20", "Dashboards", "Versions kept per dashboard", "Older versions are removed when a dashboard is saved.", false, false),

    d("search_default_range", Str, "now-24h", "Search", "Default search range", "Used when no time range is given.", false, true),
    d("search_page_size", Int(10, 500), "100", "Search", "Rows per page", "Default number of rows in the results table.", false, true),
    d("search_max_page_size", Int(10, 5000), "500", "Search", "Max rows per page", "Upper limit for one results page.", false, true),
    d("search_facet_size", Int(3, 50), "12", "Search", "Values per filter", "How many values each filter list shows.", false, false),
    d("export_max_rows", Int(100, 1000000), "100000", "Search", "CSV export limit", "Maximum rows in one export.", false, false),

    d("live_enabled", Bool, "false", "Live capture", "Enable live capture", "Off by default so it uses no resources. Even when on, a stream only runs while someone has pressed Start.", false, true),
    d("live_max_sessions", Int(1, 1000), "20", "Live capture", "Max concurrent streams", "Further viewers get an error.", false, false),
    d("live_idle_minutes", Int(1, 1440), "30", "Live capture", "Stop stream after (min)", "A stream is closed automatically after this long.", false, true),
    d("live_tick_ms", Int(200, 10000), "1000", "Live capture", "Update interval (ms)", "How often a stream sends new messages.", false, false),

    d("sql_enabled", Bool, "true", "SQL", "Enable SQL", "Turns the workbench and SQL panels on or off for everyone.", false, true),
    d("sql_timeout_seconds", Int(1, 3600), "120", "SQL", "Query timeout (s)", "Applied to queries sent to ClickHouse.", false, false),
    d("sql_max_rows", Int(10, 1000000), "10000", "SQL", "Max result rows", "Results are cut off at this size.", false, false),

    d("mask_keep_digits", Int(0, 16), "4", "Privacy", "Digits left visible when masked", "For users without the Unmask numbers permission.", false, false),

    d("session_idle_minutes", Int(1, 10080), "30", "Sessions & passwords", "Idle timeout (min)", "Sign out after this long with no requests.", false, false),
    d("session_absolute_hours", Int(1, 8760), "12", "Sessions & passwords", "Absolute timeout (h)", "Sign out this long after login, regardless of activity.", false, false),
    d("password_min_length", Int(8, 128), "12", "Sessions & passwords", "Minimum password length", "Applies when a password is set.", false, true),
    d("login_max_attempts", Int(1, 100), "5", "Sessions & passwords", "Failed logins before lockout", "Per account.", false, false),
    d("login_lockout_minutes", Int(1, 1440), "15", "Sessions & passwords", "Lockout duration (min)", "How long an account stays locked.", false, false),
    d("cookie_name", Str, "vp_session", "Sessions & passwords", "Session cookie name", "Change it when several instances share a host name.", true, false),
    d("cookie_secure", Choice(&["auto", "true", "false"]), "auto", "Sessions & passwords", "Secure cookie", "auto follows the X-Forwarded-Proto header set by a TLS proxy.", false, false),

    d("audit_retention_days", Int(1, 3650), "365", "Audit", "Audit retention (days)", "Older audit events are deleted daily.", false, false),

    d("hep_auth_key", Str, "", "Capture", "HEP auth key", "When set, HEP packets without this key are dropped. Asterisk: capture_password in hep.conf.", false, false),
    d("correlation_headers", Str, "X-CID", "Capture", "Correlation headers", "Comma-separated SIP headers that carry the other leg's Call-ID.", false, false),
    d("correlation_heuristic", Bool, "true", "Capture", "Match legs by number and time", "Joins two dialogs into one call when no header links them: same capture agent, within 3 s, the second leaves the host the first arrived at, and the last 7 digits of the called number match.", false, false),
    d("finalizer_seconds", Int(1, 60), "5", "Capture", "Call assembly interval (s)", "How often changed dialogs are turned into call records.", true, false),
    d("call_max_age_hours", Int(1, 168), "6", "Capture", "Close open calls after (h)", "A call with no signalling for this long is closed as timed out.", false, false),
    d("pdd_warning_ms", Int(100, 120000), "5000", "Capture", "High PDD threshold (ms)", "Calls above this get the High post-dial delay finding.", false, false),
    d("high_risk_prefixes", Str, "", "Capture", "High-risk prefixes", "Comma-separated normalized number prefixes that raise a toll-fraud finding.", false, false),
    d("topology_carriers", Str, "", "Topology", "Carriers", "Name=CIDR pairs, comma separated, e.g. CarrierX=203.0.113.0/24,CarrierY=198.51.100.7. Sets call direction and trunk.", false, false),
    d("topology_subscribers", Str, "", "Topology", "Subscriber groups", "Name=CIDR pairs, comma separated, e.g. Baghdad=10.10.0.0/16.", false, false),

    d("retention_messages_days", Int(1, 3650), "30", "Retention", "SIP messages (days)", "Applied to ClickHouse at startup and when a storage is selected.", true, false),
    d("retention_registrations_days", Int(1, 3650), "30", "Retention", "Registrations (days)", "Applied to ClickHouse at startup and when a storage is selected.", true, false),
    d("retention_qos_days", Int(1, 3650), "7", "Retention", "RTCP reports (days)", "Applied to ClickHouse at startup and when a storage is selected.", true, false),
    d("retention_logs_days", Int(1, 3650), "14", "Retention", "Logs (days)", "Applied to ClickHouse at startup and when a storage is selected.", true, false),
    d("retention_calls_days", Int(1, 3650), "365", "Retention", "Call records (days)", "Applied to ClickHouse at startup and when a storage is selected.", true, false),
    d("retention_metrics_days", Int(1, 3650), "90", "Retention", "Capture metrics (days)", "Applied to ClickHouse at startup and when a storage is selected.", true, false),
];

/// Process-level variables, which are read once at startup and aren't stored anywhere:
/// (name, default, description, required, group).
pub const BOOT_VARS: &[(&str, &str, &str, bool, &str)] = &[
    ("VP_DB_HOST", "", "MariaDB host name or IP", true, "Database"),
    ("VP_DB_USER", "", "MariaDB user", true, "Database"),
    ("VP_DB_PASSWORD", "", "MariaDB password. VP_DB_PASSWORD_FILE reads it from a file instead (Docker secrets)", true, "Database"),
    ("VP_DB_NAME", "voip_panel", "Database name. It must already exist; the tables are created on first start", false, "Database"),
    ("VP_DB_PORT", "3306", "MariaDB port", false, "Database"),
    ("VP_DB_URL", "", "Replaces all VP_DB_* above, e.g. mysql://user:pass@host:3306/voip_panel. VP_DB_URL_FILE also works", false, "Database"),
    ("VP_DB_SSL_MODE", "preferred", "disabled, preferred, required, verify_ca or verify_identity", false, "Database"),
    ("VP_DB_MAX_CONNECTIONS", "10", "Connection pool size", false, "Database"),
    ("VP_DB_CONNECT_RETRIES", "60", "Attempts to reach MariaDB at startup before giving up", false, "Database"),
    ("VP_DB_CONNECT_RETRY_SECONDS", "2", "Wait between those attempts", false, "Database"),
    ("VP_LISTEN", "0.0.0.0:8080", "Address and port for the web UI and API", false, "Network"),
    ("VP_HEP_UDP", "0.0.0.0:9060", "Address and port for HEP over UDP. Empty turns it off", false, "Network"),
    ("VP_HEP_TCP", "0.0.0.0:9060", "Address and port for HEP over TCP. Empty turns it off", false, "Network"),
    ("VP_TRUST_PROXY", "false", "Take the client address from X-Forwarded-For. Only behind a proxy you control", false, "Network"),
    ("VP_ADMIN_USERNAME", "admin", "Name of the first administrator", false, "First start and secrets"),
    ("VP_ADMIN_PASSWORD", "", "Creates the administrator at startup and skips the first-run screen. VP_ADMIN_PASSWORD_FILE also works", false, "First start and secrets"),
    ("VP_SETUP_TOKEN", "", "If set, the first-run screen asks for this token. Use it when the port is reachable by others before setup", false, "First start and secrets"),
    ("VP_SECRET_KEY", "", "Encrypts the ClickHouse passwords saved from the Storage page. Keep it stable: changing it makes saved passwords unreadable", false, "First start and secrets"),
    ("VP_LOG_LEVEL", "info", "trace, debug, info, warn or error", false, "Logging"),
    ("VP_LOG_FORMAT", "text", "text or json", false, "Logging"),
];

pub fn env_name(key: &str) -> String {
    format!("VP_{}", key.to_uppercase())
}

pub fn def(key: &str) -> Option<&'static Def> {
    DEFS.iter().find(|d| d.key == key)
}

pub fn validate(def: &Def, value: &str) -> Result<(), String> {
    let v = value.trim();
    match def.kind {
        Str => Ok(()),
        Int(lo, hi) => match v.parse::<i64>() {
            Ok(n) if n >= lo && n <= hi => Ok(()),
            _ => Err(format!("must be a whole number from {lo} to {hi}")),
        },
        Bool => match v {
            "true" | "false" => Ok(()),
            _ => Err("must be true or false".into()),
        },
        Choice(opts) => {
            if opts.contains(&v) {
                Ok(())
            } else {
                Err(format!("must be one of: {}", opts.join(", ")))
            }
        }
    }
}

pub struct Settings {
    env: HashMap<&'static str, String>,
    db: RwLock<HashMap<String, String>>,
}

impl Settings {
    /// Reads `VP_*` values. Invalid ones stop startup, so a typo can't be silently ignored.
    pub fn from_env() -> Result<Settings, String> {
        let mut env = HashMap::new();
        let mut errors = Vec::new();
        for def in DEFS {
            let name = env_name(def.key);
            if let Ok(value) = std::env::var(&name) {
                match validate(def, &value) {
                    Ok(()) => {
                        env.insert(def.key, value.trim().to_string());
                    }
                    Err(e) => errors.push(format!("{name} {e}")),
                }
            }
        }
        if errors.is_empty() {
            Ok(Settings { env, db: RwLock::new(HashMap::new()) })
        } else {
            Err(errors.join("\n"))
        }
    }

    pub fn load_db(&self, values: HashMap<String, String>) {
        *self.db.write().unwrap() = values;
    }

    pub fn set_db(&self, key: &str, value: Option<&str>) {
        let mut db = self.db.write().unwrap();
        match value {
            Some(v) => {
                db.insert(key.to_string(), v.to_string());
            }
            None => {
                db.remove(key);
            }
        }
    }

    pub fn locked(&self, key: &str) -> bool {
        self.env.contains_key(key)
    }

    pub fn get(&self, key: &str) -> String {
        if let Some(v) = self.env.get(key) {
            return v.clone();
        }
        if let Some(v) = self.db.read().unwrap().get(key) {
            return v.clone();
        }
        def(key).map(|d| d.default.to_string()).unwrap_or_default()
    }

    pub fn int(&self, key: &str) -> i64 {
        self.get(key).parse().unwrap_or(0)
    }

    pub fn bool(&self, key: &str) -> bool {
        self.get(key) == "true"
    }

    /// Values the browser needs for layout and defaults.
    pub fn public_json(&self) -> Value {
        let mut m = serde_json::Map::new();
        for d in DEFS.iter().filter(|d| d.public) {
            let v = self.get(d.key);
            let json = match d.kind {
                Int(..) => json!(v.parse::<i64>().unwrap_or(0)),
                Bool => json!(v == "true"),
                _ => json!(v),
            };
            m.insert(d.key.to_string(), json);
        }
        Value::Object(m)
    }

    /// The full list for the Settings page.
    pub fn admin_json(&self) -> Value {
        let db = self.db.read().unwrap();
        json!(DEFS.iter().map(|d| {
            let (kind, min, max, options): (&str, Option<f64>, Option<f64>, Vec<&str>) = match d.kind {
                Str => ("string", None, None, vec![]),
                Int(lo, hi) => ("int", Some(lo as f64), Some(hi as f64), vec![]),
                Bool => ("bool", None, None, vec![]),
                Choice(o) => ("choice", None, None, o.to_vec()),
            };
            let source = if self.env.contains_key(d.key) { "env" } else if db.contains_key(d.key) { "database" } else { "default" };
            json!({
                "key": d.key, "env": env_name(d.key), "group": d.group, "label": d.label, "help": d.help,
                "kind": kind, "min": min, "max": max, "options": options, "default": d.default,
                "value": self.get(d.key), "source": source, "restart": d.restart,
            })
        }).collect::<Vec<_>>())
    }
}

/// Reads `name`, or the contents of the file named by `name_FILE` (for Docker secrets).
pub fn env_secret(name: &str) -> Option<String> {
    if let Ok(v) = std::env::var(name)
        && !v.is_empty()
    {
        return Some(v);
    }
    let path = std::env::var(format!("{name}_FILE")).ok()?;
    std::fs::read_to_string(path).ok().map(|s| s.trim_end_matches(['\r', '\n']).to_string())
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| default.to_string())
}

fn env_num<T: std::str::FromStr>(name: &str, default: T) -> Result<T, String> {
    match std::env::var(name) {
        Ok(v) if !v.is_empty() => v.parse().map_err(|_| format!("{name} is not a valid number: {v}")),
        _ => Ok(default),
    }
}

/// Process-level configuration, which is fixed for the life of the container.
pub struct Boot {
    pub listen: SocketAddr,
    pub hep_udp: String,
    pub hep_tcp: String,
    pub db_url: Option<String>,
    pub db_host: String,
    pub db_port: u16,
    pub db_name: String,
    pub db_user: String,
    pub db_password: String,
    pub db_ssl_mode: String,
    pub db_max_connections: u32,
    pub db_connect_retries: u32,
    pub db_connect_retry_seconds: u64,
    pub setup_token: Option<String>,
    pub admin_username: String,
    pub admin_password: Option<String>,
    pub secret_key: Option<String>,
    pub log_level: String,
    pub log_format: String,
    pub trust_proxy: bool,
}

impl Boot {
    pub fn from_env() -> Result<Boot, String> {
        let db_url = env_secret("VP_DB_URL");
        let db_host = env_or("VP_DB_HOST", "");
        let db_user = env_or("VP_DB_USER", "");
        let db_password = env_secret("VP_DB_PASSWORD").unwrap_or_default();
        if db_url.is_none() {
            let mut missing = Vec::new();
            if db_host.is_empty() {
                missing.push("VP_DB_HOST");
            }
            if db_user.is_empty() {
                missing.push("VP_DB_USER");
            }
            if db_password.is_empty() {
                missing.push("VP_DB_PASSWORD (or VP_DB_PASSWORD_FILE)");
            }
            if !missing.is_empty() {
                return Err(format!("Missing required settings: {}. Alternatively set VP_DB_URL=mysql://user:pass@host:3306/voip_panel", missing.join(", ")));
            }
        }
        let listen = env_or("VP_LISTEN", "0.0.0.0:8080");
        Ok(Boot {
            listen: listen.parse().map_err(|_| format!("VP_LISTEN is not a valid address: {listen}"))?,
            hep_udp: std::env::var("VP_HEP_UDP").unwrap_or_else(|_| "0.0.0.0:9060".into()),
            hep_tcp: std::env::var("VP_HEP_TCP").unwrap_or_else(|_| "0.0.0.0:9060".into()),
            db_url,
            db_host,
            db_port: env_num("VP_DB_PORT", 3306)?,
            db_name: env_or("VP_DB_NAME", "voip_panel"),
            db_user,
            db_password,
            db_ssl_mode: env_or("VP_DB_SSL_MODE", "preferred"),
            db_max_connections: env_num("VP_DB_MAX_CONNECTIONS", 10)?,
            db_connect_retries: env_num("VP_DB_CONNECT_RETRIES", 60)?,
            db_connect_retry_seconds: env_num("VP_DB_CONNECT_RETRY_SECONDS", 2)?,
            setup_token: env_secret("VP_SETUP_TOKEN"),
            admin_username: env_or("VP_ADMIN_USERNAME", "admin"),
            admin_password: env_secret("VP_ADMIN_PASSWORD"),
            secret_key: env_secret("VP_SECRET_KEY"),
            log_level: env_or("VP_LOG_LEVEL", "info"),
            log_format: env_or("VP_LOG_FORMAT", "text"),
            trust_proxy: env_or("VP_TRUST_PROXY", "false") == "true",
        })
    }
}
