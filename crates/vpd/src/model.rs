//! Shared record types. Field names follow the ClickHouse `calls` table in §7.2 of the spec.

use serde::{Serialize, Serializer};
use serde_json::Value;

pub const SEC: i64 = 1_000_000;
pub const MIN: i64 = 60 * SEC;
pub const HOUR: i64 = 60 * MIN;
pub const DAY: i64 = 24 * HOUR;

/// Microseconds since the epoch, serialized as RFC 3339 UTC with microseconds (FR-CH-01).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ts(pub i64);

impl Serialize for Ts {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let dt = chrono::DateTime::from_timestamp_micros(self.0).unwrap_or_default();
        s.serialize_str(&dt.to_rfc3339_opts(chrono::SecondsFormat::Micros, true))
    }
}

/// `YYYY-MM-DD hh:mm:ss.ffffff` in UTC, as ClickHouse reads it in JSONEachRow.
pub fn ch_time(us: i64) -> String {
    chrono::DateTime::from_timestamp_micros(us).unwrap_or_default().format("%Y-%m-%d %H:%M:%S%.6f").to_string()
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Call {
    pub call_uid: String,
    pub state: String,
    pub start_ts: Option<Ts>,
    pub ring_ts: Option<Ts>,
    pub answer_ts: Option<Ts>,
    pub end_ts: Option<Ts>,
    pub pdd_ms: Option<u32>,
    pub setup_ms: Option<u32>,
    pub talk_ms: u64,
    pub egress_pdd_ms: Option<u32>,
    pub caller: String,
    pub caller_n: String,
    pub callee: String,
    pub callee_n: String,
    pub direction: String,
    pub ingress_group: String,
    pub egress_group: String,
    pub trunk: String,
    pub node: String,
    pub pbx: String,
    pub backend: String,
    pub capture_ids: Vec<u32>,
    pub capture_points: Vec<String>,
    pub media_node: String,
    pub final_status: u16,
    pub final_reason: String,
    pub egress_status: u16,
    pub q850_cause: u8,
    pub end_cause: String,
    pub hangup_side: String,
    pub root_call_id: String,
    pub b_call_id: Option<String>,
    pub legs: Vec<String>,
    pub link: String,
    pub codecs: Vec<String>,
    pub ua_caller: String,
    pub ua_callee: String,
    pub ua_vendor: String,
    pub ua_model: String,
    pub caller_ip: String,
    pub callee_ip: String,
    pub caller_country: String,
    pub min_mos: Option<f32>,
    pub max_loss_pct: Option<f32>,
    pub max_jitter_ms: Option<f32>,
    pub max_rtt_ms: Option<f32>,
    pub mos_estimated: bool,
    pub media_a_to_b: bool,
    pub media_b_to_a: bool,
    pub findings: Vec<String>,
    pub msg_count: u32,
}

/// Every column of `v_calls` a `Call` needs, with times as epoch microseconds.
pub const CALL_SELECT: &str = "call_uid, state, toUnixTimestamp64Micro(start_ts) AS start_us, toUnixTimestamp64Micro(ring_ts) AS ring_us,
 toUnixTimestamp64Micro(answer_ts) AS answer_us, toUnixTimestamp64Micro(end_ts) AS end_us, pdd_ms, setup_ms, talk_ms, egress_pdd_ms,
 caller, caller_n, callee, callee_n, direction, ingress_group, egress_group, trunk, node, pbx, backend, capture_ids, capture_points, media_node,
 final_status, final_reason, egress_status, q850_cause, end_cause, hangup_side, root_call_id, b_call_id, legs, link, codecs,
 ua_caller, ua_callee, ua_vendor, ua_model, caller_ip, callee_ip, caller_country, min_mos, max_loss_pct, max_jitter_ms, max_rtt_ms,
 mos_estimated, media_a_to_b, media_b_to_a, findings, msg_count";

fn text(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or("").to_string()
}

fn time(v: &Value, k: &str) -> Option<Ts> {
    v[k].as_i64().map(Ts)
}

fn opt_u32(v: &Value, k: &str) -> Option<u32> {
    v[k].as_u64().map(|x| x as u32)
}

fn opt_f32(v: &Value, k: &str) -> Option<f32> {
    v[k].as_f64().map(|x| x as f32)
}

fn strings(v: &Value, k: &str) -> Vec<String> {
    v[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

impl Call {
    pub fn from_row(v: &Value) -> Call {
        Call {
            call_uid: text(v, "call_uid"),
            state: text(v, "state"),
            start_ts: time(v, "start_us"),
            ring_ts: time(v, "ring_us"),
            answer_ts: time(v, "answer_us"),
            end_ts: time(v, "end_us"),
            pdd_ms: opt_u32(v, "pdd_ms"),
            setup_ms: opt_u32(v, "setup_ms"),
            talk_ms: v["talk_ms"].as_u64().unwrap_or(0),
            egress_pdd_ms: opt_u32(v, "egress_pdd_ms"),
            caller: text(v, "caller"),
            caller_n: text(v, "caller_n"),
            callee: text(v, "callee"),
            callee_n: text(v, "callee_n"),
            direction: text(v, "direction"),
            ingress_group: text(v, "ingress_group"),
            egress_group: text(v, "egress_group"),
            trunk: text(v, "trunk"),
            node: text(v, "node"),
            pbx: text(v, "pbx"),
            backend: text(v, "backend"),
            capture_ids: v["capture_ids"].as_array().map(|a| a.iter().filter_map(|x| x.as_u64().map(|n| n as u32)).collect()).unwrap_or_default(),
            capture_points: strings(v, "capture_points"),
            media_node: text(v, "media_node"),
            final_status: v["final_status"].as_u64().unwrap_or(0) as u16,
            final_reason: text(v, "final_reason"),
            egress_status: v["egress_status"].as_u64().unwrap_or(0) as u16,
            q850_cause: v["q850_cause"].as_u64().unwrap_or(0) as u8,
            end_cause: text(v, "end_cause"),
            hangup_side: text(v, "hangup_side"),
            root_call_id: text(v, "root_call_id"),
            b_call_id: v["b_call_id"].as_str().filter(|s| !s.is_empty()).map(str::to_string),
            legs: strings(v, "legs"),
            link: text(v, "link"),
            codecs: strings(v, "codecs"),
            ua_caller: text(v, "ua_caller"),
            ua_callee: text(v, "ua_callee"),
            ua_vendor: text(v, "ua_vendor"),
            ua_model: text(v, "ua_model"),
            caller_ip: text(v, "caller_ip"),
            callee_ip: text(v, "callee_ip"),
            caller_country: text(v, "caller_country"),
            min_mos: opt_f32(v, "min_mos"),
            max_loss_pct: opt_f32(v, "max_loss_pct"),
            max_jitter_ms: opt_f32(v, "max_jitter_ms"),
            max_rtt_ms: opt_f32(v, "max_rtt_ms"),
            mos_estimated: v["mos_estimated"].as_u64().unwrap_or(0) != 0 || v["mos_estimated"].as_bool().unwrap_or(false),
            media_a_to_b: v["media_a_to_b"].as_u64().unwrap_or(0) != 0 || v["media_a_to_b"].as_bool().unwrap_or(false),
            media_b_to_a: v["media_b_to_a"].as_u64().unwrap_or(0) != 0 || v["media_b_to_a"].as_bool().unwrap_or(false),
            findings: strings(v, "findings"),
            msg_count: v["msg_count"].as_u64().unwrap_or(0) as u32,
        }
    }
}

/// Handset vendor and model from a User-Agent string.
pub fn parse_ua(ua: &str) -> (String, String, String) {
    let lower = ua.to_ascii_lowercase();
    for vendor in ["Yealink", "Grandstream", "Fanvil", "Poly", "Polycom", "Cisco", "Linphone", "Zoiper", "MicroSIP", "Snom", "Gigaset", "Asterisk", "FreeSWITCH", "OpenSIPS", "Kamailio"] {
        if lower.contains(&vendor.to_ascii_lowercase()) {
            let parts: Vec<&str> = ua.split_whitespace().collect();
            let model = parts.iter().skip(1).find(|p| p.chars().any(|c| c.is_ascii_alphabetic())).copied().unwrap_or(vendor).trim_start_matches("SIP-");
            let firmware = parts.last().filter(|p| p.chars().next().is_some_and(|c| c.is_ascii_digit())).copied().unwrap_or("");
            return (vendor.to_string(), model.to_string(), firmware.to_string());
        }
    }
    let first = ua.split(['/', ' ']).next().unwrap_or("");
    (first.to_string(), first.to_string(), String::new())
}
