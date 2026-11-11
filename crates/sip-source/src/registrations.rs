//! Registration events and the state derived from them.

use crate::ch::Ch;
use crate::model::*;
use crate::sql::quote;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Serialize)]
pub struct RegEvent {
    pub ts: Ts,
    pub aor: String,
    pub contact: String,
    pub src_ip: String,
    pub src_port: u16,
    pub transport: String,
    pub user_agent: String,
    pub expires_requested: u32,
    pub expires_granted: u32,
    pub result: String,
    pub code: u16,
    pub rrd_ms: Option<u32>,
    pub node: String,
    pub nat: bool,
    pub challenged: bool,
    #[serde(skip)]
    pub grp: String,
}

const EVENT_COLUMNS: &str = "toUnixTimestamp64Micro(ts) AS t, aor, grp, contact, src_ip, src_port, transport, user_agent, expires_requested, expires_granted, result, code, rrd_ms, node, nat, challenged";

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or("").to_string()
}

fn flag(v: &Value) -> bool {
    v.as_bool().unwrap_or(false) || v.as_u64().unwrap_or(0) != 0
}

fn event(v: &Value) -> RegEvent {
    RegEvent {
        ts: Ts(v["t"].as_i64().unwrap_or(0)),
        aor: s(v, "aor"),
        contact: s(v, "contact"),
        src_ip: s(v, "src_ip"),
        src_port: v["src_port"].as_u64().unwrap_or(0) as u16,
        transport: s(v, "transport"),
        user_agent: s(v, "user_agent"),
        expires_requested: v["expires_requested"].as_u64().unwrap_or(0) as u32,
        expires_granted: v["expires_granted"].as_u64().unwrap_or(0) as u32,
        result: s(v, "result"),
        code: v["code"].as_u64().unwrap_or(0) as u16,
        rrd_ms: v["rrd_ms"].as_u64().map(|x| x as u32),
        node: s(v, "node"),
        nat: flag(&v["nat"]),
        challenged: flag(&v["challenged"]),
        grp: s(v, "grp"),
    }
}

/// A number's attempts in a time range, oldest first.
pub async fn events_for(ch: &Ch, number: &str, from: i64, to: i64) -> Result<Vec<RegEvent>, String> {
    let rows = ch.select(&format!("SELECT {EVENT_COLUMNS} FROM registration_events WHERE number = {} AND ts >= toDateTime64('{}', 6, 'UTC') AND ts < toDateTime64('{}', 6, 'UTC') ORDER BY ts LIMIT 5000", quote(number), ch_time(from), ch_time(to))).await?;
    Ok(rows.iter().map(event).collect())
}

#[derive(Clone, Serialize)]
pub struct Contact {
    pub contact: String,
    pub src_ip: String,
    pub src_port: u16,
    pub transport: String,
    pub user_agent: String,
    pub ua_vendor: String,
    pub ua_model: String,
    pub ua_firmware: String,
    pub country: String,
    pub nat: bool,
    pub expires_at: Ts,
    pub node: String,
}

#[derive(Clone, Serialize)]
pub struct LastFailure {
    pub ts: Ts,
    pub code: u16,
    pub result: String,
}

#[derive(Clone, Serialize)]
pub struct RegState {
    pub number: String,
    pub aor: String,
    pub group: String,
    pub status: &'static str,
    pub contact: Option<Contact>,
    pub last_success: Option<Ts>,
    pub last_failure: Option<LastFailure>,
    pub consecutive_failures: u32,
    pub failing_since: Option<Ts>,
    pub first_seen: Option<Ts>,
    pub ua_vendor: String,
    pub ua_model: String,
    pub src_ip: String,
    pub nat: bool,
}

/// Current state from a number's recent events, oldest first.
pub fn state_of(number: &str, events: &[RegEvent], now: i64) -> Option<RegState> {
    let latest = events.last()?;
    let ok = events.iter().rev().find(|e| e.code / 100 == 2);
    let bad = events.iter().rev().find(|e| e.code / 100 != 2);
    let trailing = events.iter().rev().take_while(|e| e.code / 100 != 2).count();
    let expiry = ok.map(|e| e.ts.0 + e.expires_granted as i64 * SEC);
    let live = ok.is_some_and(|e| e.expires_granted > 0) && expiry.is_some_and(|x| x > now);
    let status = match (live, trailing) {
        (true, 0..=1) => "registered",
        (false, 0) => "not_registered",
        _ => "failing",
    };
    let failing_since = (trailing > 0).then(|| events[events.len() - trailing].ts);
    let source = ok.unwrap_or(latest);
    let (vendor, model, firmware) = parse_ua(&source.user_agent);
    Some(RegState {
        number: number.to_string(),
        aor: latest.aor.clone(),
        group: source.grp.clone(),
        status,
        contact: ok.filter(|_| live).map(|e| Contact {
            contact: e.contact.clone(),
            src_ip: e.src_ip.clone(),
            src_port: e.src_port,
            transport: e.transport.clone(),
            user_agent: e.user_agent.clone(),
            ua_vendor: vendor.clone(),
            ua_model: model.clone(),
            ua_firmware: firmware.clone(),
            country: String::new(),
            nat: e.nat,
            expires_at: Ts(expiry.unwrap_or(now)),
            node: e.node.clone(),
        }),
        last_success: ok.map(|e| e.ts),
        last_failure: bad.map(|e| LastFailure { ts: e.ts, code: e.code, result: e.result.clone() }),
        consecutive_failures: trailing as u32,
        failing_since,
        first_seen: events.first().map(|e| e.ts),
        ua_vendor: vendor,
        ua_model: model,
        src_ip: source.src_ip.clone(),
        nat: source.nat,
    })
}

/// A number seen in calls but with no registration on record.
pub fn unseen(number: &str) -> RegState {
    RegState { number: number.to_string(), aor: String::new(), group: String::new(), status: "not_registered", contact: None, last_success: None, last_failure: None, consecutive_failures: 0, failing_since: None, first_seen: None, ua_vendor: String::new(), ua_model: String::new(), src_ip: String::new(), nat: false }
}

#[derive(Clone, Serialize)]
pub struct Segment {
    pub from: Ts,
    pub to: Ts,
    pub state: &'static str,
}

/// Registration state over a time range, for the state-timeline chart (FR-RG-07).
pub fn timeline(events: &[RegEvent], from: i64, to: i64) -> Vec<Segment> {
    let mut segs: Vec<Segment> = Vec::new();
    let mut push = |a: i64, b: i64, st: &'static str| {
        if b <= a {
            return;
        }
        if let Some(last) = segs.last_mut()
            && last.state == st
            && last.to.0 >= a
        {
            last.to = Ts(b);
            return;
        }
        segs.push(Segment { from: Ts(a), to: Ts(b), state: st });
    };
    let mut cursor = from;
    for (i, e) in events.iter().enumerate() {
        let next = events.get(i + 1).map(|n| n.ts.0).unwrap_or(to);
        push(cursor, e.ts.0, "unregistered");
        let good = e.code / 100 == 2 && e.expires_granted > 0;
        let until = if good { next.min(e.ts.0 + e.expires_granted as i64 * SEC) } else { next };
        push(e.ts.0, until, if good { "registered" } else if e.code / 100 == 2 { "unregistered" } else { "failing" });
        cursor = until;
    }
    push(cursor, to, "unregistered");
    segs
}

/// The latest state of every number seen in the last day, from each number's newest attempts.
pub async fn all_states(ch: &Ch, now: i64) -> Result<Vec<RegState>, String> {
    let rows = ch.select(&format!("SELECT number, {EVENT_COLUMNS} FROM registration_events WHERE ts > toDateTime64('{}', 6, 'UTC') ORDER BY number, ts DESC LIMIT 20 BY number LIMIT 500000", ch_time(now - DAY))).await?;
    let mut out = Vec::new();
    let mut batch: Vec<RegEvent> = Vec::new();
    let mut current = String::new();
    let mut finish = |number: &str, batch: &mut Vec<RegEvent>| {
        batch.reverse();
        out.extend(state_of(number, batch, now));
        batch.clear();
    };
    for r in &rows {
        let n = s(r, "number");
        if n != current {
            finish(&current, &mut batch);
            current = n;
        }
        batch.push(event(r));
    }
    finish(&current, &mut batch);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(t: i64, code: u16, granted: u32) -> RegEvent {
        RegEvent { ts: Ts(t), aor: "a".into(), contact: "c".into(), src_ip: "1.1.1.1".into(), src_port: 1, transport: "udp".into(), user_agent: "Yealink SIP-T31P 1.2".into(), expires_requested: 600, expires_granted: granted, result: "r".into(), code, rrd_ms: None, node: "n".into(), nat: false, challenged: false, grp: String::new() }
    }

    #[test]
    fn registered_until_expiry() {
        let e = [ev(0, 200, 600)];
        assert_eq!(state_of("n", &e, 10 * SEC).unwrap().status, "registered");
        assert_eq!(state_of("n", &e, 700 * SEC).unwrap().status, "not_registered");
    }

    #[test]
    fn deregistration_is_not_registered() {
        let e = [ev(0, 200, 600), ev(SEC, 200, 0)];
        assert_ne!(state_of("n", &e, 2 * SEC).unwrap().status, "registered");
    }

    #[test]
    fn repeated_failures_are_failing() {
        let e = [ev(0, 200, 600), ev(SEC, 401, 0), ev(2 * SEC, 401, 0)];
        let s = state_of("n", &e, 3 * SEC).unwrap();
        assert_eq!((s.status, s.consecutive_failures), ("failing", 2));
    }
}
