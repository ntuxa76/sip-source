//! Metric catalogue (FR-KP) and the single time engine every chart goes through (FR-CH-01..07).
//! A metric is defined once as SQL over the public views, and every panel runs that SQL.

use crate::ch::{Ch, Table};
use crate::model::*;
use crate::search;
use crate::sql::expand_macros;
use chrono::{DateTime, Offset, TimeZone};
use chrono_tz::Tz;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;

pub struct M {
    pub id: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    pub unit: &'static str,
    pub kind: &'static str,
    pub better: &'static str,
    pub min_samples: u32,
    pub family: &'static str,
    pub formula: &'static str,
}

const fn m(id: &'static str, name: &'static str, desc: &'static str, unit: &'static str, kind: &'static str, better: &'static str, min_samples: u32, family: &'static str, formula: &'static str) -> M {
    M { id, name, desc, unit, kind, better, min_samples, family, formula }
}

pub const CATALOGUE: &[M] = &[
    m("attempts", "Call attempts", "Root legs started in the bucket", "calls", "count", "none", 0, "calls", "count()"),
    m("cps", "CPS", "Average calls per second over the bucket", "/s", "rate", "none", 0, "calls", "count() / {{interval}}"),
    m("answered", "Answered calls", "Root leg received 2xx to the initial INVITE", "calls", "count", "higher", 0, "calls", "countIf(answer_ts IS NOT NULL)"),
    m("asr", "ASR", "Answered ÷ attempts", "%", "ratio", "higher", 20, "calls", "100 * countIf(answer_ts IS NOT NULL) / count()"),
    m("ner", "NER", "Answered plus user-busy, no-answer and user-reject ÷ attempts", "%", "ratio", "higher", 20, "calls", "100 * countIf(answer_ts IS NOT NULL OR final_status IN (486, 600, 480, 408, 603)) / count()"),
    m("ser", "SER (RFC 6076)", "INVITEs answered 200 ÷ (INVITEs − 3xx)", "%", "ratio", "higher", 20, "calls", "100 * countIf(final_status BETWEEN 200 AND 299) / countIf(final_status NOT BETWEEN 300 AND 399)"),
    m("seer", "SEER (RFC 6076)", "INVITEs answered 200, 480, 486, 600 or 603 ÷ (INVITEs − 3xx)", "%", "ratio", "higher", 20, "calls", "100 * countIf(final_status IN (200, 480, 486, 600, 603)) / countIf(final_status NOT BETWEEN 300 AND 399)"),
    m("isa", "ISA (RFC 6076)", "INVITEs answered 408, 500, 503 or 504 ÷ INVITEs", "%", "ratio", "lower", 20, "calls", "100 * countIf(final_status IN (408, 500, 503, 504)) / count()"),
    m("scr", "SCR (RFC 6076)", "Calls answered and ended by a normal BYE ÷ INVITEs", "%", "ratio", "higher", 20, "calls", "100 * countIf(end_cause IN ('caller_hangup', 'callee_hangup')) / count()"),
    m("pdd_p50", "PDD p50", "Initial INVITE to first 18x or final response", "ms", "latency", "lower", 10, "calls", "quantile(0.5)(pdd_ms)"),
    m("pdd_p95", "PDD p95", "Initial INVITE to first 18x or final response", "ms", "latency", "lower", 10, "calls", "quantile(0.95)(pdd_ms)"),
    m("pdd_avg", "PDD average", "Initial INVITE to first 18x or final response", "ms", "latency", "lower", 10, "calls", "avg(pdd_ms)"),
    m("setup_p95", "Setup time p95", "Initial INVITE to 2xx", "ms", "latency", "lower", 10, "calls", "quantile(0.95)(setup_ms)"),
    m("acd", "ACD", "Average talk time of answered, finished calls", "s", "latency", "higher", 10, "calls", "avgIf(talk_ms, answer_ts IS NOT NULL AND end_ts IS NOT NULL) / 1000"),
    m("traffic", "Traffic", "Total talk minutes in the bucket", "min", "count", "none", 0, "calls", "sumIf(talk_ms, end_ts IS NOT NULL) / 60000"),
    m("failures", "Failures", "Calls with a final status of 400 or above", "calls", "count", "lower", 0, "calls", "countIf(final_status >= 400)"),
    m("concurrent", "Concurrent calls", "Average answered calls active during the bucket", "calls", "gauge", "none", 0, "occupancy", "sum(active) / ({{interval}} / 60)"),
    m("erlangs", "Erlangs", "Offered traffic: talk time ÷ bucket time", "erl", "gauge", "none", 0, "occupancy", "sum(active) / ({{interval}} / 60)"),
    m("mos_p5", "MOS p5", "Fifth percentile of worst MOS per call", "mos", "quality", "higher", 10, "calls", "quantile(0.05)(min_mos)"),
    m("mos_p50", "MOS p50", "Median of worst MOS per call", "mos", "quality", "higher", 10, "calls", "quantile(0.5)(min_mos)"),
    m("mos_avg", "MOS average", "Average of worst MOS per call", "mos", "quality", "higher", 10, "calls", "avg(min_mos)"),
    m("poor_share", "Poor or bad calls", "Calls with worst MOS below 3.6", "%", "ratio", "lower", 10, "calls", "100 * countIf(min_mos < 3.6) / countIf(min_mos IS NOT NULL)"),
    m("loss_avg", "Loss average", "Average of max loss % per call", "%", "quality", "lower", 10, "calls", "avg(max_loss_pct)"),
    m("loss_p95", "Loss p95", "95th percentile of max loss % per call", "%", "quality", "lower", 10, "calls", "quantile(0.95)(max_loss_pct)"),
    m("jitter_p95", "Jitter p95", "95th percentile of max jitter per call", "ms", "quality", "lower", 10, "calls", "quantile(0.95)(max_jitter_ms)"),
    m("rtt_p95", "RTT p95", "95th percentile of max RTT per call", "ms", "quality", "lower", 10, "calls", "quantile(0.95)(max_rtt_ms)"),
    m("findings", "Findings", "Diagnostic findings raised", "findings", "count", "lower", 0, "calls", "count()"),
    m("scanner", "Scanner requests", "Requests from known scanner user agents", "calls", "count", "lower", 0, "calls", "countIf(has(findings, 'SCANNER'))"),
    m("auth_fail", "Auth failures", "Calls rejected after failed authentication, including scanners", "calls", "count", "lower", 0, "calls", "countIf(has(findings, 'AUTH_LOOP') OR has(findings, 'SCANNER'))"),
    m("reg_active", "Active registrations", "AoRs registered now", "AoRs", "gauge", "none", 0, "reg_active", "sum(active) / ({{interval}} / 60)"),
    m("reg_attempts", "Registration attempts", "REGISTER attempts per second", "/s", "rate", "none", 0, "reg", "sum(attempts) / {{interval}}"),
    m("reg_success", "Registration success", "Successful attempts ÷ attempts", "%", "ratio", "higher", 20, "reg", "100 * sum(success) / sum(attempts)"),
    m("reg_fail_ratio", "Registration failure ratio", "Failed attempts ÷ attempts", "%", "ratio", "lower", 20, "reg", "100 * (1 - sum(success) / sum(attempts))"),
    m("reg_failures", "Registration failures", "Failed attempts, by response code", "attempts", "count", "lower", 0, "reg", "sum(fail_401 + fail_403 + fail_408 + fail_503)"),
    m("reg_rrd", "RRD p95", "Registration request delay (RFC 6076), p95", "ms", "latency", "lower", 1, "reg", "max(rrd_ms)"),
    m("hep_pps", "HEP packets/s", "HEP packets received per second", "pps", "rate", "none", 0, "system", "avg(value)"),
    m("hep_drops", "HEP drops", "Packets dropped per second, by reason", "pps", "rate", "lower", 0, "system", "avg(value)"),
    m("ingest_lag", "Ingest lag", "Capture time to receive time", "ms", "gauge", "lower", 0, "system", "avg(value)"),
    m("clock_offset", "Agent clock offset", "Offset between agent and collector clocks", "ms", "gauge", "lower", 0, "system", "avg(value)"),
    m("ch_insert", "Storage write time", "Time to write one batch of captured data to the selected storage", "ms", "latency", "lower", 0, "system", "avg(value)"),
    m("sip_msgs", "SIP messages/s", "Captured SIP messages per second", "/s", "rate", "none", 0, "system", "avg(value)"),
];

pub fn find(id: &str) -> Option<&'static M> {
    CATALOGUE.iter().find(|m| m.id == id)
}

pub const DIMENSIONS: &[(&str, &str)] = &[
    ("direction", "Direction"), ("trunk", "Trunk"), ("ingress_group", "Ingress group"), ("egress_group", "Egress group"),
    ("capture_id", "Capture ID"), ("backend", "Backend type"), ("node", "Capture point"), ("status_class", "Status class"),
    ("status", "Status code"), ("q850", "Q.850 cause"), ("end_cause", "End cause"), ("codec", "Codec"),
    ("ua_vendor", "UA vendor"), ("ua_model", "UA model"), ("finding", "Finding"),
    ("prefix", "Destination prefix"), ("callee", "Destination"), ("caller", "Caller"), ("src_ip", "Source IP"),
];

#[derive(Deserialize, Clone, Default)]
pub struct Query {
    pub metric: Option<String>,
    pub metrics: Option<Vec<String>>,
    pub from: String,
    pub to: String,
    pub interval: Option<i64>,
    pub min_interval: Option<i64>,
    pub max_points: Option<usize>,
    pub group_by: Option<String>,
    pub filter: Option<String>,
    pub tz: Option<String>,
    pub top: Option<usize>,
    pub shape: Option<String>,
    pub compare: Option<bool>,
    pub order: Option<String>,
    pub var_filter: Option<String>,
    pub vars: Option<HashMap<String, Vec<String>>>,
    pub sql: Option<String>,
    pub panel_ref: Option<PanelRef>,
    pub title: Option<String>,
    pub unit: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct PanelRef {
    pub dashboard: String,
    pub panel: i64,
}

#[derive(Clone, Copy)]
pub struct Opts {
    pub settle_us: i64,
    pub max_points: usize,
    pub top_series: usize,
    pub timeout_s: u64,
    pub max_rows: usize,
}

pub fn parse_time(s: &str, now: i64) -> Option<i64> {
    let s = s.trim();
    if s == "now" {
        return Some(now);
    }
    if let Some(rest) = s.strip_prefix("now-") {
        let (num, unit) = rest.split_at(rest.len().checked_sub(1)?);
        let n: i64 = num.parse().ok()?;
        let mult = match unit {
            "s" => SEC,
            "m" => MIN,
            "h" => HOUR,
            "d" => DAY,
            _ => return None,
        };
        return Some(now - n * mult);
    }
    if let Ok(ms) = s.parse::<i64>() {
        return Some(ms * 1000);
    }
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp_micros())
}

pub fn parse_tz(s: Option<&str>) -> Tz {
    s.and_then(|t| t.parse().ok()).unwrap_or(chrono_tz::Asia::Baghdad)
}

fn tz_offset(tz: Tz, t: i64) -> i64 {
    let dt = DateTime::from_timestamp_micros(t).unwrap_or_default().naive_utc();
    tz.offset_from_utc_datetime(&dt).fix().local_minus_utc() as i64 * SEC
}

const NICE_STEPS: [i64; 14] = [1, 5, 10, 30, 60, 300, 900, 1800, 3600, 10800, 21600, 43200, 86400, 604800];

fn nice_step(min_s: f64, floor_s: i64) -> i64 {
    NICE_STEPS.iter().copied().find(|s| *s as f64 >= min_s && *s >= floor_s).unwrap_or(604800)
}

pub fn interval_label(s: i64) -> String {
    match s {
        s if s < 60 => format!("{s} s"),
        s if s < 3600 => format!("{} m", s / 60),
        s if s < 86400 => format!("{} h", s / 3600),
        s => format!("{} d", s / 86400),
    }
}

pub struct Buckets {
    pub start: i64,
    pub step: i64,
    pub n: usize,
}

impl Buckets {
    /// Buckets are aligned to local time so an hour or day bucket starts on a local boundary.
    pub fn new(from: i64, to: i64, step_s: i64, tz: Tz) -> Buckets {
        let step = step_s * SEC;
        let off = tz_offset(tz, from);
        let start = (from + off).div_euclid(step) * step - off;
        let n = (((to - start) as f64 / step as f64).ceil() as usize).max(1);
        Buckets { start, step, n }
    }

    pub fn index(&self, t: i64) -> Option<usize> {
        if t < self.start {
            return None;
        }
        let i = ((t - self.start) / self.step) as usize;
        (i < self.n).then_some(i)
    }

    pub fn starts_ms(&self) -> Vec<i64> {
        (0..self.n).map(|i| (self.start + i as i64 * self.step) / 1000).collect()
    }
}

pub fn pick_interval(from: i64, to: i64, q_interval: Option<i64>, max_points: Option<usize>, limit: usize) -> i64 {
    let range_s = ((to - from) / SEC).max(1) as f64;
    let floor = if range_s <= 6.0 * 3600.0 { 10 } else if range_s <= 90.0 * 86400.0 { 60 } else { 3600 };
    match q_interval {
        Some(i) if i > 0 => i.max(floor),
        _ => nice_step(range_s / max_points.unwrap_or(300).clamp(10, limit.max(10)) as f64, floor),
    }
}

fn dim_sql(dim: &str, findings_table: bool) -> Option<String> {
    Some(match dim {
        "status_class" => "concat(toString(intDiv(final_status, 100)), 'xx')".to_string(),
        "status" => "toString(final_status)".to_string(),
        "q850" => "toString(q850_cause)".to_string(),
        "codec" => "arrayJoin(codecs)".to_string(),
        "finding" if findings_table => "code".to_string(),
        "finding" => "arrayJoin(findings)".to_string(),
        "capture_id" => "toString(arrayJoin(capture_ids))".to_string(),
        "prefix" => "substring(callee_n, 1, 6)".to_string(),
        "callee" => "callee_n".to_string(),
        "caller" => "caller_n".to_string(),
        "src_ip" => "caller_ip".to_string(),
        other if DIMENSIONS.iter().any(|(d, _)| *d == other) => other.to_string(),
        _ => return None,
    })
}

fn system_dim(id: &str, group_by: Option<&str>) -> &'static str {
    match (id, group_by) {
        ("sip_msgs", Some("status_class")) => "status_class",
        ("sip_msgs", Some("node")) => "node",
        ("sip_msgs", _) => "method",
        _ => "default",
    }
}

/// The SQL behind a builder query, in the same dialect the SQL editor uses. This is also the
/// starting point when a panel is switched from the builder to SQL.
pub fn generated_sql(m: &M, q: &Query, shape: &str) -> String {
    if matches!(shape, "table" | "heatmap" | "histogram") {
        return String::new();
    }
    let mut conds: Vec<String> = Vec::new();
    let mut series = q.group_by.as_deref().map(str::to_string);
    let occupancy: String;
    let (table, time_col) = match m.family {
        "calls" => {
            let findings = m.id == "findings";
            if !matches!(m.id, "findings" | "scanner" | "auth_fail") {
                conds.push("NOT has(findings, 'SCANNER')".into());
            }
            if m.kind == "ratio" {
                conds.push("state NOT IN ('setup', 'ringing')".into());
            }
            series = series.and_then(|d| dim_sql(&d, findings));
            if series.as_deref() == Some("concat(toString(intDiv(final_status, 100)), 'xx')") || series.as_deref() == Some("toString(final_status)") {
                conds.push("final_status > 0".into());
            }
            (if findings { "v_findings" } else { "v_calls" }, "start_ts")
        }
        "occupancy" => {
            series = series.filter(|d| matches!(d.as_str(), "direction" | "trunk"));
            // One row per answered call per minute it was up, built from the calls themselves so
            // the dashboard's filters and the picked source apply. Calls are looked for from a
            // day before the range, which covers all but the longest.
            let filter = search::to_sql(&search::parse(q.filter.as_deref().unwrap_or("")));
            let extra = if filter.is_empty() { String::new() } else { format!(" AND {filter}") };
            occupancy = format!(
                "(SELECT toDateTime(arrayJoin(range(toUInt32(toStartOfMinute(assumeNotNull(answer_ts))), toUInt32(ifNull(end_ts, now())) + 1, 60)), 'UTC') AS bucket, direction, trunk, 1 AS active\n      FROM v_calls\n      WHERE answer_ts IS NOT NULL AND start_ts >= {{{{from}}}} - INTERVAL 1 DAY AND start_ts < {{{{to}}}} AND ifNull(end_ts, now()) >= {{{{from}}}}{extra} AND {{{{filters}}}})"
            );
            (occupancy.as_str(), "bucket")
        }
        "reg" => {
            series = series.filter(|d| d == "ingress_group").map(|_| "grp".to_string());
            ("v_kpi_reg_1m", "bucket")
        }
        "reg_active" => {
            series = series.filter(|d| d == "ingress_group").map(|_| "grp".to_string());
            ("v_reg_active_1m", "bucket")
        }
        _ => {
            conds.push(format!("metric = '{}'", m.id));
            conds.push(format!("dim = '{}'", system_dim(m.id, q.group_by.as_deref())));
            series = Some("name".to_string()).filter(|_| !matches!(m.id, "ingest_lag" | "ch_insert"));
            ("v_system_1m", "bucket")
        }
    };
    if m.family == "calls" {
        let filter = search::to_sql(&search::parse(q.filter.as_deref().unwrap_or("")));
        if !filter.is_empty() {
            conds.push(filter);
        }
        conds.push("{{filters}}".into());
    }
    let mut where_ = format!("{time_col} >= {{{{from}}}} AND {time_col} < {{{{to}}}}");
    for c in &conds {
        where_.push_str(&format!("\n  AND {c}"));
    }
    let value = m.formula;
    let samples = if m.family == "calls" { ",\n       count() AS samples" } else { "" };
    if m.id == "reg_failures" && q.group_by.as_deref() != Some("ingress_group") {
        return format!(
            "SELECT toStartOfInterval({time_col}, INTERVAL {{{{interval}}}} SECOND, {{{{tz}}}}) AS time,\n       sum(fail_401) AS `401`, sum(fail_403) AS `403`, sum(fail_408) AS `408`, sum(fail_503) AS `503`\nFROM {table}\nWHERE {where_}\nGROUP BY time\nORDER BY time"
        );
    }
    if shape == "stat" {
        return format!("SELECT {value} AS value{samples}\nFROM {table}\nWHERE {where_}");
    }
    if shape == "top" {
        let key = series.unwrap_or_else(|| "'all'".to_string());
        let dir = if q.order.as_deref() == Some("asc") { "ASC" } else { "DESC" };
        // Zero means "none" for codes such as the Q.850 cause.
        let not_empty = "\nHAVING key NOT IN ('', '0')";
        return format!("SELECT {key} AS key,\n       {value} AS value{samples}\nFROM {table}\nWHERE {where_}\nGROUP BY key{not_empty}\nORDER BY value {dir}\nLIMIT {}", q.top.unwrap_or(10));
    }
    let (series_select, series_group) = match &series {
        Some(s) => (format!(",\n       {s} AS series"), ", series"),
        None => (String::new(), ""),
    };
    format!(
        "SELECT toStartOfInterval({time_col}, INTERVAL {{{{interval}}}} SECOND, {{{{tz}}}}) AS time{series_select},\n       {value} AS value{samples}\nFROM {table}\nWHERE {where_}\nGROUP BY time{series_group}\nORDER BY time"
    )
}

pub fn metric_json(m: &M) -> Value {
    json!({"id": m.id, "name": m.name, "description": m.desc, "unit": m.unit, "kind": m.kind, "better": m.better, "min_samples": m.min_samples, "family": m.family, "formula": m.formula})
}

pub fn catalogue_json() -> Value {
    json!({
        "metrics": CATALOGUE.iter().map(metric_json).collect::<Vec<_>>(),
        "dimensions": DIMENSIONS.iter().map(|(id, name)| json!({"id": id, "name": name})).collect::<Vec<_>>(),
    })
}

fn display(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn is_num(ty: &str) -> bool {
    ty.contains("Int") || ty.contains("Float") || ty.contains("Decimal")
}

fn is_time(c: &crate::ch::Column) -> bool {
    c.ty.starts_with("DateTime") || c.ty.starts_with("Nullable(DateTime") || c.ty.starts_with("Date") || matches!(c.name.as_str(), "bucket" | "time" | "t")
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).filter(|x| x.is_finite())
}

/// Turns a result into the JSON charts expect. Builder and SQL panels share this.
#[allow(clippy::too_many_arguments)]
pub fn shape(t: &Table, q: &Query, from: i64, to: i64, tz: Tz, meta: Value, now: i64, step_s: i64, opts: &Opts, sql_text: &str, zero_fill: bool) -> Result<Value, String> {
    let shape_name = q.shape.as_deref().unwrap_or("timeseries");
    match shape_name {
        "top" => {
            let v = t.columns.iter().position(|c| is_num(&c.ty) && c.name != "samples").ok_or("The query needs a numeric column.")?;
            let k = t.columns.iter().position(|c| !is_num(&c.ty)).unwrap_or(if v == 0 { 1 } else { 0 });
            let rows: Vec<Value> = t.rows.iter().take(q.top.unwrap_or(10).max(1)).map(|r| json!({"key": display(&r[k]), "value": num(&r[v]), "samples": 0})).collect();
            Ok(json!({"shape": "top", "metric": meta, "rows": rows, "source": "the selected storage", "sql": sql_text}))
        }
        "table" => {
            let cols: Vec<Value> = t.columns.iter().skip(1).map(|c| json!({"id": c.name, "name": c.name, "unit": "", "description": c.ty})).collect();
            let rows: Vec<Value> = t.rows.iter().map(|r| {
                let mut values = serde_json::Map::new();
                for (c, v) in t.columns.iter().zip(r.iter()).skip(1) {
                    values.insert(c.name.clone(), v.clone());
                }
                json!({"key": display(&r[0]), "values": values})
            }).collect();
            Ok(json!({"shape": "table", "columns": cols, "rows": rows, "group_by": t.columns.first().map(|c| c.name.clone()).unwrap_or_default(), "sql": sql_text}))
        }
        _ => {
            let b = Buckets::new(from, to, step_s, tz);
            let buckets = b.starts_ms();
            let Some(time) = t.columns.iter().position(is_time) else {
                let v = t.columns.iter().position(|c| is_num(&c.ty) && c.name != "samples").ok_or("The query needs a numeric column.")?;
                let total = t.rows.first().and_then(|r| num(&r[v]));
                return Ok(json!({
                    "shape": "timeseries", "metric": meta, "interval_s": step_s, "interval_label": "single value", "tz": tz.to_string(),
                    "source": "the selected storage", "now": now / 1000, "buckets": buckets, "partial_from": null,
                    "total": total, "samples": 1, "previous": null, "series": [], "sql": sql_text,
                }));
            };
            let series_col = t.columns.iter().enumerate().position(|(i, c)| i != time && (c.name == "series" || !is_num(&c.ty)));
            let samples_col = t.columns.iter().position(|c| c.name == "samples");
            let value_cols: Vec<usize> = t.columns.iter().enumerate().filter(|(i, c)| *i != time && Some(*i) != series_col && Some(*i) != samples_col && is_num(&c.ty)).map(|(i, _)| i).collect();
            if value_cols.is_empty() {
                return Err("The query needs a numeric column.".into());
            }
            let mut order: Vec<String> = Vec::new();
            let mut values: HashMap<String, Vec<Option<f64>>> = HashMap::new();
            let mut samples: HashMap<String, Vec<u32>> = HashMap::new();
            for r in &t.rows {
                let Some(ms) = r[time].as_i64() else { continue };
                let Some(i) = b.index(ms * 1000) else { continue };
                for &vc in &value_cols {
                    let name = match series_col {
                        Some(sc) => display(&r[sc]),
                        None if value_cols.len() == 1 => "value".to_string(),
                        None => t.columns[vc].name.clone(),
                    };
                    if name.is_empty() {
                        continue;
                    }
                    let entry = values.entry(name.clone()).or_insert_with(|| {
                        order.push(name.clone());
                        vec![if zero_fill { Some(0.0) } else { None }; b.n]
                    });
                    entry[i] = num(&r[vc]);
                    samples.entry(name).or_insert_with(|| vec![0; b.n])[i] = samples_col.and_then(|c| r[c].as_u64()).unwrap_or(1) as u32;
                }
            }
            // Nothing happened yet: draw a flat zero rather than an empty chart.
            if order.is_empty() && zero_fill {
                order.push("value".to_string());
                values.insert("value".to_string(), vec![Some(0.0); b.n]);
                samples.insert("value".to_string(), vec![0; b.n]);
            }
            let total_of = |n: &String| values[n].iter().flatten().sum::<f64>();
            // A series that is zero everywhere only adds a legend entry.
            if zero_fill && order.len() > 1 {
                order.retain(|n| total_of(n) != 0.0);
            }
            order.sort_by(|a, b| total_of(b).total_cmp(&total_of(a)));
            let limit = q.top.unwrap_or(opts.top_series).clamp(1, opts.top_series);
            let folded = order.len() > limit;
            let tail: Vec<String> = if folded { order.split_off(limit) } else { vec![] };
            let settle = now - opts.settle_us;
            let partial_from = (0..b.n).find(|i| b.start + (*i as i64 + 1) * b.step > settle);
            let last_settled = partial_from.unwrap_or(b.n).max(1);
            let total = order.first().and_then(|n| values[n].iter().take(last_settled).rev().flatten().next().copied());
            let mut series: Vec<Value> = order.iter().map(|n| json!({"name": n, "values": values[n], "samples": samples[n]})).collect();
            if !tail.is_empty() && zero_fill {
                let mut other = vec![0.0; b.n];
                for n in &tail {
                    for (i, v) in values[n].iter().enumerate() {
                        other[i] += v.unwrap_or(0.0);
                    }
                }
                series.push(json!({"name": "other", "values": other, "samples": vec![1; b.n]}));
            }
            Ok(json!({
                "shape": "timeseries", "metric": meta, "interval_s": step_s, "interval_label": format!("{} buckets", interval_label(step_s)), "tz": tz.to_string(),
                "source": "the selected storage", "now": now / 1000, "buckets": buckets, "partial_from": partial_from,
                "total": total, "samples": 1, "previous": null, "series": series, "sql": sql_text,
            }))
        }
    }
}

fn var_conditions(q: &Query) -> String {
    search::to_sql(&search::parse(q.var_filter.as_deref().unwrap_or("")))
}

async fn fetch(ch: &Ch, text: &str, q: &Query, from: i64, to: i64, step: i64, tz: Tz, opts: &Opts) -> Result<Table, String> {
    let expanded = expand_macros(text, from / 1000, to / 1000, step, tz, &var_conditions(q), &q.vars.clone().unwrap_or_default());
    ch.query_readonly(&expanded, opts.timeout_s, opts.max_rows).await
}

pub async fn run_sql(ch: &Ch, q: &Query, text: &str, title: &str, unit: &str, now: i64, opts: &Opts) -> Result<Value, String> {
    let from = parse_time(&q.from, now).ok_or("Bad time range")?;
    let to = parse_time(&q.to, now).ok_or("Bad time range")?.min(now);
    if to <= from {
        return Err("The time range is empty.".into());
    }
    let tz = parse_tz(q.tz.as_deref());
    let stat = q.shape.as_deref() == Some("stat");
    let step = if stat { ((to - from) / SEC).max(1) } else { pick_interval(from, to, q.interval, q.max_points, opts.max_points).max(q.min_interval.unwrap_or(0)) };
    let t = fetch(ch, text, q, from, to, step, tz, opts).await?;
    let meta = json!({"id": "sql", "name": title, "description": "Custom SQL", "unit": unit, "kind": "gauge", "better": "none", "min_samples": 0, "family": "sql", "formula": ""});
    shape(&t, q, from, to, tz, meta, now, step, opts, text, false)
}

/// Runs a builder panel: the metric's SQL against the selected storage.
pub async fn run(ch: &Ch, now: i64, q: &Query, opts: &Opts) -> Result<Value, String> {
    let from = parse_time(&q.from, now).ok_or("Bad time range")?;
    let to = parse_time(&q.to, now).ok_or("Bad time range")?.min(now);
    if to <= from {
        return Err("The time range is empty.".into());
    }
    let tz = parse_tz(q.tz.as_deref());
    let shape_name = q.shape.as_deref().unwrap_or("timeseries");
    if shape_name == "table" {
        return table(ch, q, from, to, tz, opts).await;
    }
    let id = q.metric.as_deref().ok_or("A metric is required.")?;
    let m = find(id).ok_or_else(|| format!("Unknown metric {id}"))?;
    match shape_name {
        "heatmap" => heatmap(ch, q, m, from, to, tz, opts).await,
        "histogram" => histogram(ch, q, m, from, to, tz, opts).await,
        "top" => {
            let text = generated_sql(m, q, "top");
            let t = fetch(ch, &text, q, from, to, ((to - from) / SEC).max(1), tz, opts).await?;
            shape(&t, q, from, to, tz, metric_json(m), now, 60, opts, &text, false)
        }
        _ => timeseries(ch, q, m, from, to, tz, now, opts, shape_name).await,
    }
}

fn is_gauge(m: &M) -> bool {
    matches!(m.family, "occupancy" | "reg_active" | "system")
}

#[allow(clippy::too_many_arguments)]
async fn timeseries(ch: &Ch, q: &Query, m: &M, from: i64, to: i64, tz: Tz, now: i64, opts: &Opts, shape_name: &str) -> Result<Value, String> {
    let step = pick_interval(from, to, q.interval, q.max_points, opts.max_points).max(q.min_interval.unwrap_or(0));
    let text = generated_sql(m, q, "timeseries");
    let t = fetch(ch, &text, q, from, to, step, tz, opts).await?;
    let mut plain = q.clone();
    plain.shape = None;
    // No row means nothing happened: for these, that is a real zero rather than a gap.
    let zero_fill = matches!(m.kind, "count" | "rate") || matches!(m.family, "occupancy" | "reg_active");
    let mut out = shape(&t, &plain, from, to, tz, metric_json(m), now, step, opts, &text, zero_fill)?;
    if shape_name == "stat" || q.compare.unwrap_or(false) {
        // A range total is its own query, so a ratio is never an average of ratios.
        let span = to - from;
        let range_s = (span / SEC).max(1);
        let stat_sql = generated_sql(m, q, "stat");
        let single = |t: &Table| t.rows.first().and_then(|r| num(&r[0]));
        if is_gauge(m) {
            // The latest complete bucket. A gauge has no late-arriving rows to settle.
            let sum_last = |v: &Value| -> Option<f64> {
                let series = v["series"].as_array()?;
                let n = v["buckets"].as_array()?.len().saturating_sub(1).max(1);
                let latest = (0..n).rev().find_map(|i| {
                    let vals: Vec<f64> = series.iter().filter_map(|s| s["values"][i].as_f64()).collect();
                    (!vals.is_empty()).then(|| if m.kind == "latency" { vals.iter().sum::<f64>() / vals.len() as f64 } else { vals.iter().sum::<f64>() })
                });
                latest.or_else(|| zero_fill.then_some(0.0))
            };
            out["total"] = json!(sum_last(&out));
            let prev = fetch(ch, &text, q, from - span, from, step, tz, opts).await?;
            let prev_shape = shape(&prev, &plain, from - span, from, tz, metric_json(m), now, step, opts, &text, zero_fill)?;
            out["previous"] = json!(sum_last(&prev_shape));
        } else {
            let t = fetch(ch, &stat_sql, q, from, to, range_s, tz, opts).await?;
            out["total"] = json!(single(&t));
            let p = fetch(ch, &stat_sql, q, from - span, from, range_s, tz, opts).await?;
            out["previous"] = json!(single(&p).filter(|x| *x != 0.0));
        }
        out["sql"] = json!(stat_sql);
    }
    Ok(out)
}

async fn table(ch: &Ch, q: &Query, from: i64, to: i64, tz: Tz, opts: &Opts) -> Result<Value, String> {
    let dim = q.group_by.as_deref().ok_or("group_by is required")?;
    let ids = q.metrics.clone().ok_or("metrics is required")?;
    let metrics: Vec<&M> = ids.iter().map(|i| find(i).ok_or(format!("Unknown metric {i}"))).collect::<Result<_, _>>()?;
    let range_s = ((to - from) / SEC).max(1);
    let key = dim_sql(dim, false).ok_or("Unknown breakdown")?;
    let exprs: Vec<String> = metrics.iter().filter(|m| m.family == "calls").map(|m| format!("{} AS {}", m.formula, m.id)).collect();
    let filter = search::to_sql(&search::parse(q.filter.as_deref().unwrap_or("")));
    let sql_text = format!("SELECT {key} AS key, count() AS total_rows, {} FROM v_calls WHERE start_ts >= {{{{from}}}} AND start_ts < {{{{to}}}} AND NOT has(findings, 'SCANNER') AND {} AND {{{{filters}}}} GROUP BY key HAVING key != '' ORDER BY total_rows DESC LIMIT {}", exprs.join(", "), if filter.is_empty() { "1 = 1".to_string() } else { filter }, q.top.unwrap_or(12));
    let t = fetch(ch, &sql_text, q, from, to, range_s, tz, opts).await?;
    let mut rows: Vec<(String, serde_json::Map<String, Value>)> = t.rows.iter().map(|r| {
        let mut m = serde_json::Map::new();
        for (c, v) in t.columns.iter().zip(r.iter()).skip(2) {
            m.insert(c.name.clone(), json!(num(v)));
        }
        (display(&r[0]), m)
    }).collect();
    if metrics.iter().any(|m| m.family == "occupancy") && matches!(dim, "trunk" | "direction") {
        let occ = fetch(ch, &format!("SELECT {dim} AS key, sum(active) / greatest(dateDiff('minute', {{{{from}}}}, {{{{to}}}}), 1) AS v FROM v_kpi_concurrency_1m WHERE bucket >= {{{{from}}}} AND bucket < {{{{to}}}} GROUP BY key"), q, from, to, range_s, tz, opts).await?;
        let by_key: HashMap<String, f64> = occ.rows.iter().map(|r| (display(&r[0]), num(&r[1]).unwrap_or(0.0))).collect();
        for (k, m) in &mut rows {
            for id in metrics.iter().filter(|m| m.family == "occupancy") {
                m.insert(id.id.to_string(), json!(by_key.get(k).copied().unwrap_or(0.0)));
            }
        }
    }
    Ok(json!({
        "shape": "table",
        "group_by": dim,
        "columns": metrics.iter().map(|m| metric_json(m)).collect::<Vec<_>>(),
        "rows": rows.iter().map(|(k, m)| json!({"key": k, "values": m})).collect::<Vec<_>>(),
    }))
}

async fn heatmap(ch: &Ch, q: &Query, m: &M, from: i64, to: i64, tz: Tz, opts: &Opts) -> Result<Value, String> {
    let dim = q.group_by.as_deref().unwrap_or("status");
    let key = dim_sql(dim, false).ok_or("Unknown breakdown")?;
    let step = pick_interval(from, to, q.interval, Some(q.max_points.unwrap_or(72).min(120)), opts.max_points);
    let b = Buckets::new(from, to, step, tz);
    let text = format!("SELECT toStartOfInterval(start_ts, INTERVAL {{{{interval}}}} SECOND, {{{{tz}}}}) AS time, {key} AS series, count() AS value FROM v_calls WHERE start_ts >= {{{{from}}}} AND start_ts < {{{{to}}}} AND final_status > 0 AND {{{{filters}}}} GROUP BY time, series");
    let t = fetch(ch, &text, q, from, to, step, tz, opts).await?;
    let mut totals: HashMap<String, u64> = HashMap::new();
    for r in &t.rows {
        *totals.entry(display(&r[1])).or_default() += r[2].as_u64().unwrap_or(0);
    }
    let mut keys: Vec<String> = totals.keys().cloned().collect();
    keys.sort_by_key(|k| std::cmp::Reverse(totals[k]));
    keys.truncate(14);
    keys.sort();
    let cells: Vec<Value> = t.rows.iter().filter_map(|r| {
        let x = b.index(r[0].as_i64()? * 1000)?;
        let y = keys.iter().position(|k| *k == display(&r[1]))?;
        Some(json!([x, y, r[2].as_u64().unwrap_or(0)]))
    }).collect();
    Ok(json!({
        "shape": "heatmap", "metric": metric_json(m), "buckets": b.starts_ms(), "keys": keys, "cells": cells,
        "interval_s": step, "interval_label": format!("{} buckets", interval_label(step)), "tz": tz.to_string(), "source": "the selected storage",
    }))
}

async fn histogram(ch: &Ch, q: &Query, m: &M, from: i64, to: i64, tz: Tz, opts: &Opts) -> Result<Value, String> {
    let (col, lo, width, n) = if m.id.starts_with("mos") { ("min_mos", 1.0, 0.25, 14usize) } else { ("pdd_ms", 0.0, 500.0, 20usize) };
    let text = format!("SELECT least(toInt32(floor(({col} - {lo}) / {width})), {}) AS i, count() AS c FROM v_calls WHERE start_ts >= {{{{from}}}} AND start_ts < {{{{to}}}} AND {col} IS NOT NULL AND {{{{filters}}}} GROUP BY i", n - 1);
    let t = fetch(ch, &text, q, from, to, 60, tz, opts).await?;
    let mut bins = vec![0u64; n];
    for r in &t.rows {
        let i = r[0].as_i64().unwrap_or(0).max(0) as usize;
        if i < n {
            bins[i] += r[1].as_u64().unwrap_or(0);
        }
    }
    Ok(json!({
        "shape": "histogram", "metric": metric_json(m), "source": "the selected storage",
        "bins": bins.iter().enumerate().map(|(i, c)| json!({"from": lo + i as f64 * width, "to": lo + (i + 1) as f64 * width, "count": c})).collect::<Vec<_>>(),
    }))
}

pub fn sql_literal_time(us: i64) -> String {
    format!("toDateTime64('{}', 6, 'UTC')", ch_time(us))
}
