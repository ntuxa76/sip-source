//! Anomaly detection with learned baselines. Every five minutes the last 30 minutes of traffic,
//! overall, per trunk and per source, are compared with what is normal for that hour: the same
//! hour on the same weekday when there are three weeks of history, the same hour on other days
//! when there are three days, otherwise the last two days. A value outside the normal band opens
//! an anomaly in MariaDB; it closes after two clean runs. No thresholds have to be set.

use crate::api::{App, now_us};
use crate::auth::{User, db_error};
use crate::ch::Ch;
use crate::db::{iso, utc_now};
use axum::Json;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

const HOUR: i64 = 3600;
const DAY: i64 = 24 * HOUR;
/// The window that is judged, ending a minute ago so recent calls have settled.
const WINDOW_MIN: i64 = 30;
const HISTORY_DAYS: i64 = 22;

/// One hour of traffic for one dimension value.
#[derive(Clone, Default, Debug)]
struct Stats {
    attempts: f64,
    finished: f64,
    answered: f64,
    fail5: f64,
    pdd: Option<f64>,
    pdd_n: f64,
    mos: Option<f64>,
    mos_n: f64,
    regs: f64,
    regfail: f64,
}

type Key = (String, String);

#[derive(Clone, Copy, PartialEq)]
enum Metric {
    Volume,
    Asr,
    Fail5,
    Pdd,
    Mos,
    RegFail,
}

impl Metric {
    const ALL: [Metric; 6] = [Metric::Volume, Metric::Asr, Metric::Fail5, Metric::Pdd, Metric::Mos, Metric::RegFail];

    fn id(self) -> &'static str {
        match self {
            Metric::Volume => "volume",
            Metric::Asr => "asr",
            Metric::Fail5 => "fail5xx",
            Metric::Pdd => "pdd",
            Metric::Mos => "mos",
            Metric::RegFail => "regfail",
        }
    }

    /// The metric's value for an hour, or for the window scaled to an hour. None when there is
    /// too little traffic for it to mean anything.
    fn value(self, s: &Stats, scale: f64, min_calls: f64) -> Option<f64> {
        match self {
            Metric::Volume => Some(s.attempts * scale),
            Metric::Asr => (s.finished >= min_calls).then(|| s.answered / s.finished * 100.0),
            Metric::Fail5 => (s.finished >= min_calls).then(|| s.fail5 / s.finished * 100.0),
            Metric::Pdd => (s.pdd_n >= min_calls).then_some(s.pdd).flatten(),
            Metric::Mos => (s.mos_n >= (min_calls / 2.0).max(3.0)).then_some(s.mos).flatten(),
            Metric::RegFail => (s.regs > 0.0 || s.regfail > 0.0).then_some(s.regfail * scale),
        }
    }
}

struct Finding {
    dim: String,
    value: String,
    metric: Metric,
    up: bool,
    severity: &'static str,
    current: f64,
    baseline: f64,
    band: (f64, f64),
    samples: usize,
    basis: &'static str,
    series: Vec<(i64, f64)>,
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    if n == 0 {
        0.0
    } else if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// Median and a robust spread (1.4826 × median absolute deviation, which equals the standard
/// deviation for normal data).
fn centre(values: &[f64]) -> (f64, f64) {
    let mut v = values.to_vec();
    let m = median(&mut v);
    let mut dev: Vec<f64> = values.iter().map(|x| (x - m).abs()).collect();
    (m, 1.4826 * median(&mut dev))
}

/// Picks the hours that describe "normal" for the hour starting at `now_hour`.
fn baseline_hours(hours: &[(i64, Stats)], now_hour: i64) -> (Vec<&Stats>, &'static str) {
    let hod = (now_hour / HOUR).rem_euclid(24);
    let dow = (now_hour / DAY + 4).rem_euclid(7);
    let past: Vec<&(i64, Stats)> = hours.iter().filter(|(t, _)| *t < now_hour - HOUR).collect();
    let same_week: Vec<&Stats> = past.iter().filter(|(t, _)| (t / HOUR).rem_euclid(24) == hod && (t / DAY + 4).rem_euclid(7) == dow).map(|(_, s)| s).collect();
    if same_week.len() >= 3 {
        return (same_week, "this hour on the same weekday");
    }
    let same_hour: Vec<&Stats> = past.iter().filter(|(t, _)| (t / HOUR).rem_euclid(24) == hod).map(|(_, s)| s).collect();
    if same_hour.len() >= 3 {
        return (same_hour, "this hour on previous days");
    }
    let recent: Vec<&Stats> = past.iter().filter(|(t, _)| *t >= now_hour - 2 * DAY).map(|(_, s)| s).collect();
    if recent.len() >= 12 {
        return (recent, "the last two days");
    }
    (Vec::new(), "")
}

/// Judges one metric. Returns whether it is too high or too low, and the band it should be in.
fn judge(metric: Metric, cur: f64, base: f64, spread: f64, finding_floor: f64) -> Option<(bool, &'static str, (f64, f64))> {
    match metric {
        Metric::Volume => {
            let lo = base - (3.0 * spread).max(0.5 * base);
            let hi = base + (4.0 * spread).max(base).max(20.0);
            if base >= 20.0 && cur < lo {
                Some((false, if cur < 0.2 * base { "crit" } else { "serious" }, (lo.max(0.0), hi)))
            } else if cur > hi && cur >= 30.0 {
                Some((true, "warn", (lo.max(0.0), hi)))
            } else {
                None
            }
        }
        Metric::Asr => {
            let lo = base - (3.0 * spread).max(15.0);
            (cur < lo).then_some((false, if base - cur >= 30.0 { "crit" } else { "serious" }, (lo.max(0.0), 100.0)))
        }
        Metric::Fail5 => {
            let hi = base + (3.0 * spread).max(10.0);
            (cur > hi).then_some((true, if cur - base >= 30.0 { "crit" } else { "serious" }, (0.0, hi)))
        }
        Metric::Pdd => {
            let hi = (base * 1.5).max(base + (3.0 * spread).max(1000.0));
            (cur > hi).then_some((true, "warn", (0.0, hi)))
        }
        Metric::Mos => {
            let lo = base - (3.0 * spread).max(0.4);
            (cur < lo).then_some((false, if cur < 3.1 { "serious" } else { "warn" }, (lo, 5.0)))
        }
        Metric::RegFail => {
            let hi = base + (4.0 * spread).max(finding_floor).max(base);
            (cur > hi).then_some((true, if cur > 5.0 * hi { "serious" } else { "warn" }, (0.0, hi)))
        }
    }
}

fn f(v: &Value, k: &str) -> f64 {
    v[k].as_f64().or_else(|| v[k].as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0)
}

fn opt(v: &Value, k: &str) -> Option<f64> {
    let x = &v[k];
    if x.is_null() {
        return None;
    }
    x.as_f64().filter(|n| n.is_finite())
}

/// Calls and registrations per dimension value, per hour or for one window.
async fn collect(ch: &Ch, from: &str, to: &str, by_hour: bool) -> Result<HashMap<Key, HashMap<i64, Stats>>, String> {
    let bucket = if by_hour { "toUnixTimestamp(toStartOfHour(start_ts))" } else { "0" };
    let calls = ch.select(&format!(
        "SELECT d.1 AS dim, d.2 AS val, {bucket} AS h, count() AS attempts,
                countIf(state NOT IN ('setup', 'ringing', 'answered')) AS finished,
                countIf(answer_ts IS NOT NULL AND state NOT IN ('setup', 'ringing')) AS answered,
                countIf(final_status >= 500 AND state NOT IN ('setup', 'ringing', 'answered')) AS fail5,
                quantileIf(0.5)(pdd_ms, pdd_ms IS NOT NULL) AS pdd, countIf(pdd_ms IS NOT NULL) AS pdd_n,
                avgIf(min_mos, min_mos IS NOT NULL) AS mos, countIf(min_mos IS NOT NULL) AS mos_n
         FROM v_calls ARRAY JOIN [('all', ''), ('trunk', trunk), ('source', if(empty(agent_ips), '', agent_ips[1]))] AS d
         WHERE start_ts >= {from} AND start_ts < {to} AND (d.1 = 'all' OR d.2 != '')
         GROUP BY dim, val, h"
    )).await?;
    let bucket = if by_hour { "toUnixTimestamp(toStartOfHour(ts))" } else { "0" };
    let regs = ch.select(&format!(
        "SELECT d.1 AS dim, d.2 AS val, {bucket} AS h, count() AS regs,
                countIf(result IN ('auth_failed', 'rejected', 'no_response')) AS regfail
         FROM registration_events ARRAY JOIN [('all', ''), ('source', agent_ip)] AS d
         WHERE ts >= {from} AND ts < {to} AND (d.1 = 'all' OR d.2 != '')
         GROUP BY dim, val, h"
    )).await?;
    let mut out: HashMap<Key, HashMap<i64, Stats>> = HashMap::new();
    let key = |r: &Value| (r["dim"].as_str().unwrap_or("").to_string(), r["val"].as_str().unwrap_or("").to_string());
    for r in &calls {
        let s = out.entry(key(r)).or_default().entry(f(r, "h") as i64).or_default();
        s.attempts = f(r, "attempts");
        s.finished = f(r, "finished");
        s.answered = f(r, "answered");
        s.fail5 = f(r, "fail5");
        s.pdd = opt(r, "pdd");
        s.pdd_n = f(r, "pdd_n");
        s.mos = opt(r, "mos");
        s.mos_n = f(r, "mos_n");
    }
    for r in &regs {
        let s = out.entry(key(r)).or_default().entry(f(r, "h") as i64).or_default();
        s.regs = f(r, "regs");
        s.regfail = f(r, "regfail");
    }
    Ok(out)
}

pub struct Detector {
    /// Hourly history per dimension value, refreshed once an hour.
    history: HashMap<Key, Vec<(i64, Stats)>>,
    loaded_hour: i64,
    /// Hours of traffic seen overall, to tell the UI how far learning has got.
    pub learned_hours: usize,
    /// Consecutive clean runs per open anomaly.
    misses: HashMap<i64, u32>,
}

impl Detector {
    fn new() -> Detector {
        Detector { history: HashMap::new(), loaded_hour: 0, learned_hours: 0, misses: HashMap::new() }
    }

    async fn run_once(&mut self, app: &App, ch: &Ch) -> Result<(), String> {
        let now = now_us() / 1_000_000;
        let hour = now - now.rem_euclid(HOUR);
        if self.loaded_hour != hour {
            let from = format!("toDateTime({}) ", hour - HISTORY_DAYS * DAY);
            let to = format!("toDateTime({hour})");
            let raw = collect(ch, &from, &to, true).await?;
            self.history = raw.into_iter().map(|(k, m)| {
                let mut v: Vec<(i64, Stats)> = m.into_iter().collect();
                v.sort_by_key(|e| e.0);
                (k, v)
            }).collect();
            self.learned_hours = self.history.get(&("all".to_string(), String::new())).map(|v| v.len()).unwrap_or(0);
            self.loaded_hour = hour;
        }
        let window = collect(ch, &format!("now() - INTERVAL {} MINUTE", WINDOW_MIN + 1), "now() - INTERVAL 1 MINUTE", false).await?;
        let min_calls = app.settings.int("anomaly_min_calls").max(3) as f64;
        let scale = 60.0 / WINDOW_MIN as f64;

        let mut found = Vec::new();
        let empty = Vec::new();
        // Values with history but nothing in the window still matter: their traffic stopped.
        let mut keys: Vec<&Key> = window.keys().chain(self.history.keys()).collect();
        keys.sort();
        keys.dedup();
        for key in keys {
            let hours = self.history.get(key).unwrap_or(&empty);
            let (base_hours, basis) = baseline_hours(hours, hour);
            if base_hours.is_empty() {
                continue;
            }
            let zero = Stats::default();
            let cur_stats = window.get(key).and_then(|m| m.get(&0)).unwrap_or(&zero);
            for metric in Metric::ALL {
                let Some(cur) = metric.value(cur_stats, scale, min_calls) else { continue };
                let samples: Vec<f64> = base_hours.iter().filter_map(|s| metric.value(s, 1.0, (min_calls / 2.0).max(3.0))).collect();
                if samples.len() < 3 {
                    continue;
                }
                let (base, spread) = centre(&samples);
                let Some((up, severity, band)) = judge(metric, cur, base, spread, 10.0) else { continue };
                // The last day of this metric, for the chart.
                let mut series: Vec<(i64, f64)> = hours.iter().filter(|(t, _)| *t >= hour - DAY).filter_map(|(t, s)| metric.value(s, 1.0, 1.0).map(|v| (*t, v))).collect();
                series.push((now, cur));
                found.push(Finding { dim: key.0.clone(), value: key.1.clone(), metric, up, severity, current: cur, baseline: base, band, samples: samples.len(), basis, series });
            }
        }
        // A trunk or source that carries nearly all traffic would only repeat what was found for
        // all traffic.
        let all_key = ("all".to_string(), String::new());
        let volume = |k: &Key| self.history.get(k).map(|h| h.iter().map(|(_, s)| s.attempts + s.regs).sum::<f64>()).unwrap_or(0.0);
        let all_volume = volume(&all_key);
        let overall: Vec<(&'static str, bool)> = found.iter().filter(|x| x.dim == "all").map(|x| (x.metric.id(), x.up)).collect();
        found.retain(|x| {
            x.dim == "all" || !overall.contains(&(x.metric.id(), x.up)) || volume(&(x.dim.clone(), x.value.clone())) < 0.9 * all_volume
        });
        self.store(app, found).await.map_err(|e| e.to_string())
    }

    async fn store(&mut self, app: &App, found: Vec<Finding>) -> Result<(), sqlx::Error> {
        let open = sqlx::query("SELECT id, dim, dim_value, metric, worst_value, direction FROM anomalies WHERE resolved_at IS NULL").fetch_all(&app.db).await?;
        // A metric that turns the other way is a new anomaly; the old one closes.
        type OpenKey = (String, String, String, bool);
        let mut by_key: HashMap<OpenKey, (i64, f64, bool)> = open
            .iter()
            .map(|r| {
                let up = r.get::<String, _>("direction") == "up";
                ((r.get("dim"), r.get("dim_value"), r.get("metric"), up), (r.get("id"), r.get("worst_value"), up))
            })
            .collect();
        let now = utc_now();
        for x in &found {
            let details = json!({
                "band": [x.band.0, x.band.1], "basis": x.basis, "samples": x.samples,
                "series": x.series.iter().map(|(t, v)| json!([t * 1000, v])).collect::<Vec<_>>(),
            })
            .to_string();
            let k = (x.dim.clone(), x.value.clone(), x.metric.id().to_string(), x.up);
            if let Some((id, worst, up)) = by_key.remove(&k) {
                let worst = if up { worst.max(x.current) } else { worst.min(x.current) };
                sqlx::query("UPDATE anomalies SET current_value = ?, baseline = ?, worst_value = ?, severity = IF(FIELD(severity, 'warn', 'serious', 'crit') >= FIELD(?, 'warn', 'serious', 'crit'), severity, ?), last_seen = ?, details = ? WHERE id = ?")
                    .bind(x.current).bind(x.baseline).bind(worst).bind(x.severity).bind(x.severity).bind(now).bind(&details).bind(id)
                    .execute(&app.db).await?;
                self.misses.remove(&id);
            } else {
                sqlx::query("INSERT INTO anomalies (dim, dim_value, metric, direction, severity, current_value, baseline, worst_value, started_at, last_seen, details) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
                    .bind(&x.dim).bind(&x.value).bind(x.metric.id()).bind(if x.up { "up" } else { "down" }).bind(x.severity)
                    .bind(x.current).bind(x.baseline).bind(x.current).bind(now).bind(now).bind(&details)
                    .execute(&app.db).await?;
                tracing::info!("anomaly: {} {} {} is {:.1}, normal {:.1}", x.dim, x.value, x.metric.id(), x.current, x.baseline);
            }
        }
        // What wasn't found this time is closed after two clean runs in a row.
        for (_, (id, _, _)) in by_key {
            let n = self.misses.entry(id).or_insert(0);
            *n += 1;
            if *n >= 2 {
                sqlx::query("UPDATE anomalies SET resolved_at = ? WHERE id = ?").bind(now).bind(id).execute(&app.db).await?;
                self.misses.remove(&id);
            }
        }
        sqlx::query("DELETE FROM anomalies WHERE resolved_at < ?").bind(now - chrono::Duration::days(90)).execute(&app.db).await?;
        Ok(())
    }
}

pub async fn run(app: Arc<App>) {
    let mut detector = Detector::new();
    tokio::time::sleep(Duration::from_secs(60)).await;
    let mut tick = tokio::time::interval(Duration::from_secs(300));
    loop {
        tick.tick().await;
        let learned = detector.learned_hours;
        app.anomaly_learned_hours.store(learned, std::sync::atomic::Ordering::Relaxed);
        if !app.settings.bool("anomaly_enabled") {
            continue;
        }
        let Some(ch) = app.storage.read().unwrap().clone() else { continue };
        if let Err(e) = detector.run_once(&app, &ch).await {
            tracing::warn!("anomaly detection: {e}");
        }
        app.anomaly_learned_hours.store(detector.learned_hours, std::sync::atomic::Ordering::Relaxed);
    }
}

#[derive(Deserialize)]
pub struct ListQuery {
    state: Option<String>,
    days: Option<i64>,
}

pub async fn list(State(a): State<Arc<App>>, user: User, Query(q): Query<ListQuery>) -> Response {
    if let Err(r) = user.need("dashboards_view") {
        return r;
    }
    let days = q.days.unwrap_or(7).clamp(1, 90);
    let open_only = q.state.as_deref() == Some("open");
    let sql = if open_only {
        "SELECT * FROM anomalies WHERE resolved_at IS NULL ORDER BY FIELD(severity, 'crit', 'serious', 'warn'), started_at DESC LIMIT 200"
    } else {
        "SELECT * FROM anomalies WHERE resolved_at IS NULL OR started_at >= ? ORDER BY resolved_at IS NOT NULL, started_at DESC LIMIT 500"
    };
    let mut query = sqlx::query(sql);
    if !open_only {
        query = query.bind(utc_now() - chrono::Duration::days(days));
    }
    let rows = match query.fetch_all(&a.db).await {
        Ok(r) => r,
        Err(e) => return db_error(e),
    };
    // Trunk and source values are shown with their alias when the source has one.
    let agents = a.ingest.agents.read().unwrap();
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            let dim: String = r.get("dim");
            let value: String = r.get("dim_value");
            let label = if dim == "source" { agents.get(&value).map(|g| g.label()).unwrap_or_else(|| value.clone()) } else { value.clone() };
            json!({
                "id": r.get::<i64, _>("id"),
                "dim": dim, "value": value, "label": label,
                "metric": r.get::<String, _>("metric"),
                "direction": r.get::<String, _>("direction"),
                "severity": r.get::<String, _>("severity"),
                "current": r.get::<f64, _>("current_value"),
                "baseline": r.get::<f64, _>("baseline"),
                "worst": r.get::<f64, _>("worst_value"),
                "started_at": iso(r.get("started_at")),
                "last_seen": iso(r.get("last_seen")),
                "resolved_at": r.get::<Option<chrono::NaiveDateTime>, _>("resolved_at").map(iso),
                "details": serde_json::from_str::<Value>(&r.get::<String, _>("details")).unwrap_or(Value::Null),
            })
        })
        .collect();
    let learned = a.anomaly_learned_hours.load(std::sync::atomic::Ordering::Relaxed);
    Json(json!({
        "enabled": a.settings.bool("anomaly_enabled"),
        "learned_hours": learned,
        "items": items,
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn robust_centre() {
        let (m, s) = centre(&[10.0, 11.0, 9.0, 10.0, 50.0]);
        assert_eq!(m, 10.0);
        assert!(s < 2.0, "one outlier doesn't widen the band: {s}");
    }

    #[test]
    fn asr_drop_is_found() {
        assert!(judge(Metric::Asr, 30.0, 62.0, 3.0, 10.0).is_some_and(|(up, sev, _)| !up && sev == "crit"));
        assert!(judge(Metric::Asr, 58.0, 62.0, 3.0, 10.0).is_none());
    }

    #[test]
    fn stopped_traffic_is_critical() {
        assert!(judge(Metric::Volume, 0.0, 120.0, 10.0, 10.0).is_some_and(|(_, sev, _)| sev == "crit"));
        assert!(judge(Metric::Volume, 5.0, 8.0, 1.0, 10.0).is_none(), "quiet hours are left alone");
    }

    #[test]
    fn baseline_prefers_same_hour() {
        let day0 = 1_700_000_000 - 1_700_000_000 % DAY;
        let now = day0 + 10 * DAY + 14 * HOUR;
        let hours: Vec<(i64, Stats)> = (0..10 * 24).map(|h| (day0 + h * HOUR, Stats { attempts: if h % 24 == 14 { 100.0 } else { 5.0 }, ..Stats::default() })).collect();
        let (b, basis) = baseline_hours(&hours, now);
        assert_eq!(basis, "this hour on previous days");
        assert!(b.iter().all(|s| s.attempts == 100.0));
    }
}
