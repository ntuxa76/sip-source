//! SQL helpers: macro expansion, the documented public views, and quoting.

use chrono::DateTime;
use chrono_tz::Tz;
use serde_json::{Value, json};
use std::collections::HashMap;

pub const CALL_COLUMNS: &[(&str, &str, &str)] = &[
    ("call_uid", "String", "Stable call identifier"),
    ("state", "String", "setup, ringing, answered, completed, failed, cancelled, timeout"),
    ("start_ts", "DateTime64", "First INVITE, UTC"),
    ("ring_ts", "Nullable(DateTime64)", "First ringing"),
    ("answer_ts", "Nullable(DateTime64)", "Answer time"),
    ("end_ts", "Nullable(DateTime64)", "End of the call"),
    ("pdd_ms", "Nullable(UInt32)", "Post-dial delay"),
    ("setup_ms", "Nullable(UInt32)", "INVITE to answer"),
    ("talk_ms", "UInt64", "Talk time"),
    ("egress_pdd_ms", "Nullable(UInt32)", "Post-dial delay on the egress leg"),
    ("caller", "String", "Calling number as signalled"),
    ("caller_n", "String", "Normalized calling number"),
    ("callee", "String", "Called number as signalled"),
    ("callee_n", "String", "Normalized called number"),
    ("direction", "String", "onnet, outbound, inbound, unknown"),
    ("ingress_group", "String", "Topology group the call came from"),
    ("egress_group", "String", "Topology group the call left to"),
    ("trunk", "String", "Carrier name"),
    ("node", "String", "Capture point of the first INVITE"),
    ("pbx", "String", "B2BUA capture point"),
    ("backend", "String", "asterisk, freeswitch, opensips, kamailio or unknown"),
    ("media_node", "String", "Capture point that reported media"),
    ("capture_ids", "Array(UInt32)", "HEP capture IDs that saw the call"),
    ("capture_points", "Array(String)", "Capture point names"),
    ("agent_ips", "Array(String)", "Addresses of the HEP senders that saw the call"),
    ("final_status", "UInt16", "Status as the caller saw it"),
    ("final_reason", "String", "Reason phrase"),
    ("egress_status", "UInt16", "Status from the egress leg"),
    ("q850_cause", "UInt8", "Q.850 cause"),
    ("end_cause", "String", "Why the call ended"),
    ("hangup_side", "String", "caller, callee or network"),
    ("root_call_id", "String", "A-leg Call-ID"),
    ("b_call_id", "String", "B-leg Call-ID"),
    ("legs", "Array(String)", "All Call-IDs of this call"),
    ("link", "String", "How legs were joined: header or inferred"),
    ("codecs", "Array(String)", "Negotiated codecs"),
    ("ua_caller", "String", "Caller user agent"),
    ("ua_callee", "String", "Callee user agent"),
    ("ua_vendor", "String", "Handset vendor"),
    ("ua_model", "String", "Handset model"),
    ("caller_ip", "String", "Caller address"),
    ("callee_ip", "String", "Callee address"),
    ("min_mos", "Nullable(Float32)", "Worst MOS"),
    ("max_loss_pct", "Nullable(Float32)", "Max loss %"),
    ("max_jitter_ms", "Nullable(Float32)", "Max jitter"),
    ("max_rtt_ms", "Nullable(Float32)", "Max RTT"),
    ("findings", "Array(String)", "Diagnostic codes"),
    ("msg_count", "UInt32", "SIP messages captured"),
];

pub fn schema_json() -> Value {
    let cols = |list: &[(&str, &str, &str)]| list.iter().map(|(n, t, d)| json!({"name": n, "type": t, "description": d})).collect::<Vec<_>>();
    let calls = cols(CALL_COLUMNS);
    let mut findings = cols(&[("code", "String", "Finding code")]);
    findings.extend(calls.clone());
    json!([
        {"name": "v_calls", "description": "One row per correlated call", "columns": calls},
        {"name": "v_findings", "description": "One row per diagnostic finding, with the call's columns", "columns": findings},
        {"name": "v_messages", "description": "Every captured SIP message", "columns": cols(&[("ts", "DateTime64", "Capture time"), ("capture_id", "UInt32", "HEP capture ID"), ("capture_point", "String", "Source name"), ("agent_ip", "String", "HEP sender address"), ("call_id", "String", "SIP Call-ID"), ("method", "String", "Request method"), ("status", "UInt16", "Response code"), ("from_user", "String", "From user"), ("to_user", "String", "To user"), ("src_ip", "String", "Source"), ("dst_ip", "String", "Destination"), ("user_agent", "String", "User agent"), ("payload", "String", "Raw message")])},
        {"name": "v_registrations", "description": "One row per registration attempt", "columns": cols(&[("ts", "DateTime64", "Attempt time"), ("aor", "String", "Address of record"), ("number", "String", "Normalized number"), ("result", "String", "registered, refreshed, deregistered, auth_failed, rejected, no_response"), ("code", "UInt16", "Final code"), ("rrd_ms", "Nullable(UInt32)", "Registration delay"), ("src_ip", "String", "Source address"), ("user_agent", "String", "User agent"), ("grp", "String", "Topology group")])},
        {"name": "v_qos_reports", "description": "RTCP media reports", "columns": cols(&[("ts", "DateTime64", "Report time"), ("call_id", "String", "SIP Call-ID"), ("ssrc", "String", "Stream"), ("loss_pct", "Float32", "Packet loss"), ("jitter_ms", "Float32", "Jitter"), ("rtt_ms", "Nullable(Float32)", "Round trip"), ("mos", "Nullable(Float32)", "MOS")])},
        {"name": "v_logs", "description": "Log lines sent over HEP", "columns": cols(&[("ts", "DateTime64", "Time"), ("call_id", "String", "SIP Call-ID"), ("node", "String", "Capture point"), ("severity", "String", "Level"), ("message", "String", "Text")])},
        {"name": "v_kpi_concurrency_1m", "description": "Answered calls active per minute", "columns": cols(&[("bucket", "DateTime", "Minute"), ("direction", "String", "Direction"), ("trunk", "String", "Carrier"), ("active", "UInt64", "Active calls")])},
        {"name": "v_kpi_reg_1m", "description": "Registration attempts per minute", "columns": cols(&[("bucket", "DateTime", "Minute"), ("grp", "String", "Group"), ("attempts", "UInt64", "Attempts"), ("success", "UInt64", "Successful"), ("fail_401", "UInt64", "401"), ("fail_403", "UInt64", "403"), ("fail_408", "UInt64", "408 or no response"), ("fail_503", "UInt64", "5xx"), ("rrd_ms", "Float64", "Delay p95")])},
        {"name": "v_reg_active_1m", "description": "Registered AoRs per minute", "columns": cols(&[("bucket", "DateTime", "Minute"), ("grp", "String", "Group"), ("active", "UInt32", "Registered")])},
        {"name": "v_system_1m", "description": "Capture health per minute", "columns": cols(&[("bucket", "DateTime", "Minute"), ("metric", "String", "hep_pps, sip_msgs, hep_drops, …"), ("dim", "String", "default, method, status_class or node"), ("name", "String", "Agent, method or reason"), ("value", "Float64", "Value")])},
    ])
}

pub fn quote(v: &str) -> String {
    format!("'{}'", v.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// Fills `{{from}}`, `{{to}}`, `{{interval}}`, `{{tz}}`, `{{filters}}` and any dashboard variables.
pub fn expand_macros(sql: &str, from_ms: i64, to_ms: i64, interval_s: i64, tz: Tz, filters: &str, vars: &HashMap<String, Vec<String>>) -> String {
    let ts = |ms: i64| format!("toDateTime64('{}', 3, 'UTC')", DateTime::from_timestamp_millis(ms).unwrap_or_default().format("%Y-%m-%d %H:%M:%S%.3f"));
    let mut out = sql
        .replace("{{from}}", &ts(from_ms))
        .replace("{{to}}", &ts(to_ms))
        .replace("{{interval}}", &interval_s.to_string())
        .replace("{{tz}}", &format!("'{tz}'"))
        .replace("{{filters}}", if filters.is_empty() { "1 = 1" } else { filters });
    for (name, values) in vars {
        // Plain decimal numbers go in as numbers; everything else is a quoted string.
        let numeric = |v: &str| !v.is_empty() && v.len() < 32 && v.trim_start_matches('-').split_once('.').map_or(v.trim_start_matches('-').chars().all(|c| c.is_ascii_digit()), |(i, f)| !i.is_empty() && i.chars().chain(f.chars()).all(|c| c.is_ascii_digit()) && !f.is_empty());
        let list = values.iter().map(|v| if numeric(v) { v.clone() } else { quote(v) }).collect::<Vec<_>>().join(", ");
        out = out.replace(&format!("{{{{{name}}}}}"), &list);
    }
    out
}

pub const LIBRARY: &[(&str, &str, &str, &str)] = &[
    ("top-dest-failures", "Failures", "Top 20 destinations by failures", "SELECT substring(callee_n, 1, 6) AS prefix, count() AS failures\nFROM v_calls\nWHERE start_ts >= {{from}} AND final_status >= 400\nGROUP BY prefix\nORDER BY failures DESC\nLIMIT 20"),
    ("ack-missing-node", "Failures", "Calls with ACK missing per node", "SELECT node, count() AS calls\nFROM v_calls\nWHERE start_ts >= {{from}} AND has(findings, 'ACK_MISSING')\nGROUP BY node\nORDER BY calls DESC"),
    ("q850-causes", "Failures", "Q.850 causes", "SELECT q850_cause, count() AS calls\nFROM v_calls\nWHERE start_ts >= {{from}} AND final_status >= 400\nGROUP BY q850_cause\nORDER BY calls DESC"),
    ("recent-failed", "Failures", "Latest failed calls", "SELECT start_ts, caller_n, callee_n, final_status, final_reason\nFROM v_calls\nWHERE start_ts >= {{from}} AND final_status >= 400\nORDER BY start_ts DESC\nLIMIT 50"),
    ("asr-trunk", "Carriers", "ASR by trunk", "SELECT trunk, count() AS attempts,\n       round(100 * countIf(answer_ts IS NOT NULL) / attempts, 1) AS asr\nFROM v_calls\nWHERE start_ts >= {{from}} AND trunk != ''\nGROUP BY trunk\nORDER BY attempts DESC"),
    ("capture-volume", "Capture", "Calls per capture ID", "SELECT arrayJoin(capture_ids) AS capture_id, count() AS calls\nFROM v_calls\nWHERE start_ts >= {{from}}\nGROUP BY capture_id\nORDER BY calls DESC"),
    ("backend-asr", "Capture", "ASR by backend", "SELECT backend, count() AS calls,\n       round(100 * countIf(answer_ts IS NOT NULL) / calls, 1) AS asr\nFROM v_calls\nWHERE start_ts >= {{from}}\nGROUP BY backend\nORDER BY calls DESC"),
    ("sip-methods", "Capture", "SIP messages by method", "SELECT method, count() AS messages\nFROM v_messages\nWHERE ts >= {{from}} AND is_request\nGROUP BY method\nORDER BY messages DESC"),
    ("mos-ua", "Media", "MOS by UA model", "SELECT ua_model, round(avg(min_mos), 2) AS mos, count() AS calls\nFROM v_calls\nWHERE start_ts >= {{from}} AND min_mos IS NOT NULL\nGROUP BY ua_model\nORDER BY mos ASC"),
    ("codec-mix", "Media", "Codec mix", "SELECT arrayJoin(codecs) AS codec, count() AS calls\nFROM v_calls\nWHERE start_ts >= {{from}}\nGROUP BY codec\nORDER BY calls DESC"),
    ("top-callers", "Traffic", "Top 20 callers by volume", "SELECT caller_n, count() AS calls, round(sum(talk_ms) / 60000) AS minutes\nFROM v_calls\nWHERE start_ts >= {{from}}\nGROUP BY caller_n\nORDER BY calls DESC\nLIMIT 20"),
    ("calls-hour", "Traffic", "Calls per hour", "SELECT toStartOfHour(start_ts, {{tz}}) AS hour, count() AS calls\nFROM v_calls\nWHERE start_ts >= {{from}}\nGROUP BY hour\nORDER BY hour"),
    ("findings-count", "Diagnostics", "Findings by code", "SELECT code, count() AS findings\nFROM v_findings\nWHERE start_ts >= {{from}}\nGROUP BY code\nORDER BY findings DESC"),
    ("failing-registrations", "Registrations", "Failing registrations", "SELECT aor, argMax(code, ts) AS last_code, count() AS attempts\nFROM v_registrations\nWHERE ts >= {{from}} AND code >= 300\nGROUP BY aor\nORDER BY attempts DESC\nLIMIT 50"),
];

pub fn library() -> Value {
    json!(LIBRARY.iter().map(|(id, category, title, sql)| json!({"id": id, "category": category, "title": title, "sql": sql})).collect::<Vec<_>>())
}
