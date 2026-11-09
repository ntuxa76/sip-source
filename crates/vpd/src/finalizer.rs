//! Turns captured SIP messages into call records. Every few seconds it re-assembles the dialogs that
//! changed, joins legs of the same call, and writes a new version of each record to ClickHouse.

use crate::api::{App, now_us};
use crate::ch::Ch;
use crate::config::Settings;
use crate::ingest::private_ip;
use crate::model::*;
use crate::search::normalize;
use crate::sql::quote;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const NS: Uuid = Uuid::from_u128(0x7e57_9a11_0000_4000_8000_766f_6970_706e);
const BATCH: usize = 2000;

#[derive(Clone)]
pub struct Group {
    pub name: String,
    pub carrier: bool,
}

pub struct Topology(Vec<(u32, u32, Group)>);

fn parse_ipv4(s: &str) -> Option<u32> {
    let p: Vec<u32> = s.split('.').map(|x| x.parse().ok()).collect::<Option<Vec<_>>>()?;
    (p.len() == 4).then(|| (p[0] << 24) | (p[1] << 16) | (p[2] << 8) | p[3])
}

impl Topology {
    /// Reads `Name=CIDR` pairs, comma separated. A name can repeat for several ranges.
    pub fn parse(s: &Settings) -> Topology {
        let mut out = Vec::new();
        for (key, carrier) in [("topology_carriers", true), ("topology_subscribers", false)] {
            for item in s.get(key).split(',').map(str::trim).filter(|x| !x.is_empty()) {
                let Some((name, cidr)) = item.split_once('=') else { continue };
                let (net, bits) = cidr.trim().split_once('/').unwrap_or((cidr.trim(), "32"));
                let (Some(net), Ok(bits)) = (parse_ipv4(net), bits.parse::<u32>()) else { continue };
                let mask = if bits == 0 { 0 } else { u32::MAX << (32 - bits.min(32)) };
                out.push((net & mask, mask, Group { name: name.trim().to_string(), carrier }));
            }
        }
        out.sort_by_key(|e| std::cmp::Reverse(e.1));
        Topology(out)
    }

    pub fn classify(&self, ip: &str) -> Option<Group> {
        let addr = parse_ipv4(ip)?;
        self.0.iter().find(|(net, mask, _)| addr & mask == *net).map(|e| e.2.clone())
    }
}

#[derive(Clone, Debug, Default)]
struct M {
    ts: i64,
    capture_id: u32,
    capture_point: String,
    src_ip: String,
    src_port: u16,
    dst_ip: String,
    dst_port: u16,
    is_request: bool,
    method: String,
    status: u16,
    reason: String,
    cseq: u32,
    cseq_method: String,
    from_user: String,
    from_tag: String,
    to_tag: String,
    ruri_user: String,
    via_branch: String,
    user_agent: String,
    codecs: Vec<String>,
    sdp_ip: String,
    sdp_dir: String,
    q850: u8,
    x_cid: String,
    hep_cid: String,
}

const MSG_COLUMNS: &str = "toUnixTimestamp64Micro(ts) AS ts_us, capture_id, capture_point, src_ip, src_port, dst_ip, dst_port, call_id, is_request, method, status, reason, cseq_num, cseq_method, from_user, from_tag, to_tag, ruri_user, via_branch, user_agent, codecs, sdp_ip, sdp_dir, q850_cause, x_cid, hep_cid";

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or("").to_string()
}

fn truthy(v: &Value) -> bool {
    v.as_bool().unwrap_or(false) || v.as_u64().unwrap_or(0) != 0
}

fn message(v: &Value) -> (String, M) {
    (s(v, "call_id"), M {
        ts: v["ts_us"].as_i64().unwrap_or(0),
        capture_id: v["capture_id"].as_u64().unwrap_or(0) as u32,
        capture_point: s(v, "capture_point"),
        src_ip: s(v, "src_ip"),
        src_port: v["src_port"].as_u64().unwrap_or(0) as u16,
        dst_ip: s(v, "dst_ip"),
        dst_port: v["dst_port"].as_u64().unwrap_or(0) as u16,
        is_request: truthy(&v["is_request"]),
        method: s(v, "method"),
        status: v["status"].as_u64().unwrap_or(0) as u16,
        reason: s(v, "reason"),
        cseq: v["cseq_num"].as_u64().unwrap_or(0) as u32,
        cseq_method: s(v, "cseq_method"),
        from_user: s(v, "from_user"),
        from_tag: s(v, "from_tag"),
        to_tag: s(v, "to_tag"),
        ruri_user: s(v, "ruri_user"),
        via_branch: s(v, "via_branch"),
        user_agent: s(v, "user_agent"),
        codecs: v["codecs"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
        sdp_ip: s(v, "sdp_ip"),
        sdp_dir: s(v, "sdp_dir"),
        q850: v["q850_cause"].as_u64().unwrap_or(0) as u8,
        x_cid: s(v, "x_cid"),
        hep_cid: s(v, "hep_cid"),
    })
}

/// One dialog's messages, with the same message seen at several capture points folded together.
struct Dialog {
    id: String,
    msgs: Vec<M>,
    retrans: u32,
    response_retrans: HashMap<(u16, u32), u32>,
    capture_ids: BTreeSet<u32>,
    capture_points: BTreeSet<String>,
    raw_count: u32,
}

impl Dialog {
    fn build(id: String, all: Vec<M>) -> Dialog {
        let mut seen: HashMap<(String, u16, String, u16, String, u16, u32, String), u32> = HashMap::new();
        let mut d = Dialog { id, msgs: Vec::new(), retrans: 0, response_retrans: HashMap::new(), capture_ids: BTreeSet::new(), capture_points: BTreeSet::new(), raw_count: all.len() as u32 };
        for m in all {
            d.capture_ids.insert(m.capture_id);
            d.capture_points.insert(m.capture_point.clone());
            let key = (m.src_ip.clone(), m.src_port, m.dst_ip.clone(), m.dst_port, format!("{}{}", m.method, m.cseq_method), m.status, m.cseq, m.via_branch.clone());
            match seen.get_mut(&key) {
                Some(capture) if *capture != m.capture_id => {}
                Some(_) => {
                    if m.is_request {
                        d.retrans = d.retrans.saturating_add(1);
                    } else {
                        *d.response_retrans.entry((m.status, m.cseq)).or_default() += 1;
                    }
                }
                None => {
                    seen.insert(key, m.capture_id);
                    d.msgs.push(m);
                }
            }
        }
        d
    }

    fn first_invite(&self) -> Option<&M> {
        self.msgs.iter().find(|m| m.is_request && m.method == "INVITE" && m.to_tag.is_empty())
    }
}

#[derive(Default)]
struct Leg {
    start: i64,
    ring: Option<i64>,
    answer: Option<i64>,
    end: Option<i64>,
    last: i64,
    first_response: Option<i64>,
    status: u16,
    reason: String,
    q850: u8,
    cancelled: bool,
    bye_from_caller: Option<bool>,
    any_provisional: bool,
    ack_missing: bool,
    auth_challenges: u32,
    held: bool,
}

fn analyze(d: &Dialog) -> Leg {
    let invite = d.first_invite().expect("checked by caller");
    let mut leg = Leg { start: invite.ts, ..Leg::default() };
    leg.last = d.msgs.iter().map(|m| m.ts).max().unwrap_or(invite.ts);
    let initial: Vec<u32> = d.msgs.iter().filter(|m| m.is_request && m.method == "INVITE" && m.to_tag.is_empty()).map(|m| m.cseq).collect();
    let last_cseq = initial.iter().copied().max().unwrap_or(invite.cseq);
    for m in &d.msgs {
        if m.is_request {
            match m.method.as_str() {
                "CANCEL" => leg.cancelled = true,
                "BYE" if leg.bye_from_caller.is_none() => {
                    leg.bye_from_caller = Some(m.from_tag == invite.from_tag);
                    leg.end = Some(m.ts);
                    if m.q850 > 0 {
                        leg.q850 = m.q850;
                    }
                }
                "INVITE" if !m.to_tag.is_empty() && matches!(m.sdp_dir.as_str(), "sendonly" | "inactive") => leg.held = true,
                _ => {}
            }
            continue;
        }
        if m.cseq_method == "BYE" && m.status / 100 == 2 {
            leg.end = Some(m.ts);
        }
        if m.cseq_method != "INVITE" || !initial.contains(&m.cseq) {
            continue;
        }
        if m.status == 401 || m.status == 407 {
            leg.auth_challenges += 1;
        }
        if m.status > 100 && m.status < 200 {
            leg.any_provisional = true;
            leg.ring.get_or_insert(m.ts);
        }
        if m.status >= 200 && m.cseq == last_cseq && leg.first_response.is_none() {
            leg.first_response = Some(m.ts);
        }
        if m.status / 100 == 2 && leg.answer.is_none() {
            leg.answer = Some(m.ts);
            leg.status = m.status;
            leg.reason = m.reason.clone();
        } else if m.status >= 300 && m.cseq == last_cseq && leg.answer.is_none() && leg.status == 0 {
            leg.status = m.status;
            leg.reason = m.reason.clone();
            leg.q850 = m.q850;
            leg.end = Some(m.ts);
        }
    }
    let has_ack = d.msgs.iter().any(|m| m.is_request && m.method == "ACK" && initial.contains(&m.cseq));
    let ok_retrans = d.response_retrans.iter().filter(|((status, seq), _)| status / 100 == 2 && initial.contains(seq)).map(|(_, n)| *n).max().unwrap_or(0);
    leg.ack_missing = leg.answer.is_some() && !has_ack && ok_retrans >= 3;
    leg
}

struct Qos {
    min_mos: Option<f32>,
    loss: f32,
    jitter: f32,
    rtt: Option<f32>,
    estimated: bool,
    node: String,
}

fn agent_skewed(app: &App, ids: &BTreeSet<u32>) -> bool {
    let agents = app.ingest.agents.read().unwrap();
    ids.iter().any(|id| agents.get(id).is_some_and(|a| a.clock_offset_ms.abs() > 1000.0))
}

/// Builds one call from its dialogs, earliest first. Returns the row and when to look again.
fn assemble(app: &App, topo: &Topology, legs: &[&Dialog], qos: &HashMap<String, Qos>, link: &str, now: i64) -> (Value, Option<i64>) {
    let root_dialog = legs[0];
    let egress_dialog = legs[legs.len() - 1];
    let invite = root_dialog.first_invite().expect("checked by caller");
    let root = analyze(root_dialog);
    let egress = analyze(egress_dialog);
    let egress_invite = egress_dialog.first_invite().expect("checked by caller");
    let max_age = app.settings.int("call_max_age_hours") * HOUR;

    let caller_n = normalize(&invite.from_user);
    let callee_n = normalize(&invite.ruri_user);
    let ingress = topo.classify(&invite.src_ip);
    let egress_group = topo.classify(&egress_invite.dst_ip);
    let (direction, trunk) = match (&ingress, &egress_group) {
        (Some(i), _) if i.carrier => ("inbound", i.name.clone()),
        (_, Some(e)) if e.carrier => ("outbound", e.name.clone()),
        (Some(_), Some(_)) => ("onnet", String::new()),
        _ => ("unknown", String::new()),
    };

    // The caller's view wins, but when only the egress leg has answered, use what's known.
    let answer = root.answer.or(egress.answer);
    let bye = root.bye_from_caller.or(egress.bye_from_caller);
    let end = root.end.or(egress.end);
    let last = legs.iter().map(|d| analyze(d).last).max().unwrap_or(root.last);
    let status = if root.status != 0 { root.status } else { egress.status };
    let reason = if root.status != 0 { root.reason.clone() } else { egress.reason.clone() };
    let q850 = if root.q850 > 0 { root.q850 } else { egress.q850 };
    let any_prov = legs.iter().any(|d| analyze(d).any_provisional);
    let ack_missing = legs.iter().any(|d| analyze(d).ack_missing);
    let challenges: u32 = legs.iter().map(|d| analyze(d).auth_challenges).sum();
    let held = legs.iter().any(|d| analyze(d).held);
    let cancelled = root.cancelled || status == 487;

    let mut recheck = None;
    let (state, end_cause, hangup, end_ts): (&str, &str, &str, Option<i64>) = if let Some(_a) = answer {
        match (bye, end) {
            (Some(by_caller), Some(e)) => {
                let (cause, side) = if ack_missing { ("ack_timeout", "network") } else if by_caller { ("caller_hangup", "caller") } else { ("callee_hangup", "callee") };
                ("completed", cause, side, Some(e))
            }
            _ if now - last > max_age => ("timeout", "network_timeout", "network", Some(last)),
            _ => {
                recheck = Some(last + max_age + MIN);
                ("answered", "", "unknown", None)
            }
        }
    } else if status >= 300 {
        let cause = match status {
            487 => "cancelled",
            486 | 600 => "busy",
            480 => "no_answer",
            408 if any_prov => "no_answer",
            408 => "network_timeout",
            404 | 484 | 410 => "unreachable",
            _ => "rejected",
        };
        (if cancelled { "cancelled" } else { "failed" }, cause, if cancelled { "caller" } else { "callee" }, end.or(Some(last)))
    } else if !any_prov && now - root.start > 32 * SEC {
        ("timeout", "network_timeout", "network", Some(last))
    } else if !any_prov {
        recheck = Some(root.start + 33 * SEC);
        ("setup", "", "unknown", None)
    } else if now - last > max_age {
        ("timeout", "network_timeout", "network", Some(last))
    } else {
        recheck = Some(last + max_age + MIN);
        ("ringing", "", "unknown", None)
    };

    let ring = root.ring.or(egress.ring);
    let pdd = ring.or(root.first_response).map(|t| ((t - root.start) / 1000).max(0));
    let egress_pdd = egress.ring.or(egress.first_response).map(|t| ((t - egress.start) / 1000).max(0));
    let setup = answer.map(|a| ((a - root.start) / 1000).max(0));
    let talk = match (answer, end_ts) {
        (Some(a), Some(e)) => ((e - a) / 1000).max(0),
        (Some(a), None) => ((now - a) / 1000).max(0),
        _ => 0,
    };

    let mut capture_ids: BTreeSet<u32> = BTreeSet::new();
    let mut capture_points: BTreeSet<String> = BTreeSet::new();
    for d in legs {
        capture_ids.extend(d.capture_ids.iter().copied());
        capture_points.extend(d.capture_points.iter().cloned());
    }
    let all: Vec<&M> = legs.iter().flat_map(|d| d.msgs.iter()).collect();
    let ua_callee = egress_dialog.msgs.iter().find(|m| !m.is_request && m.status > 100 && !m.user_agent.is_empty()).map(|m| m.user_agent.clone()).unwrap_or_default();
    let backend_of = |ua: &str| {
        let l = ua.to_ascii_lowercase();
        ["freeswitch", "asterisk", "kamailio", "opensips"].iter().find(|b| l.contains(**b)).copied()
    };
    let backend = all.iter().filter_map(|m| backend_of(&m.user_agent)).find(|b| matches!(*b, "asterisk" | "freeswitch")).or(all.iter().filter_map(|m| backend_of(&m.user_agent)).next()).unwrap_or("unknown");
    let codecs: Vec<String> = all.iter().rev().find(|m| !m.is_request && !m.codecs.is_empty()).map(|m| m.codecs.iter().filter(|c| *c != "telephone-event").take(1).cloned().collect()).unwrap_or_default();
    let (vendor, model, _) = parse_ua(&invite.user_agent);

    let mut mos: Option<f32> = None;
    let (mut loss, mut jitter, mut rtt, mut estimated, mut any_qos, mut media_node) = (None::<f32>, None::<f32>, None::<f32>, false, false, String::new());
    for d in legs {
        if let Some(q) = qos.get(&d.id) {
            any_qos = true;
            if let Some(m) = q.min_mos {
                mos = Some(mos.map_or(m, |x: f32| x.min(m)));
            }
            loss = Some(loss.map_or(q.loss, |x: f32| x.max(q.loss)));
            jitter = Some(jitter.map_or(q.jitter, |x: f32| x.max(q.jitter)));
            if let Some(r) = q.rtt {
                rtt = Some(rtt.map_or(r, |x: f32| x.max(r)));
            }
            estimated |= q.estimated;
            media_node.clone_from(&q.node);
        }
    }

    let pdd_slow = pdd.is_some_and(|p| p > app.settings.int("pdd_warning_ms"));
    let talk_after_answer = talk as f64 / 1000.0;
    let mut f: Vec<&str> = Vec::new();
    let mut add = |cond: bool, code: &'static str| {
        if cond {
            f.push(code);
        }
    };
    let finished = end_ts.is_some();
    add(state == "timeout" && answer.is_none() && !any_prov, "NO_FINAL_RESPONSE");
    add(ack_missing, "ACK_MISSING");
    add(finished && answer.is_some() && (30.0..=33.0).contains(&talk_after_answer), "DROP_AFTER_32S");
    add(challenges >= 3 && answer.is_none(), "AUTH_LOOP");
    add(matches!(status, 488 | 606), "NO_COMMON_CODEC");
    add(finished && !invite.sdp_ip.is_empty() && private_ip(&invite.sdp_ip) && !private_ip(&invite.src_ip), "PRIVATE_IP_IN_SDP");
    add(pdd_slow, "HIGH_PDD");
    add(legs.iter().any(|d| d.retrans >= 3), "RETRANSMISSIONS");
    add(mos.is_some_and(|m| m < 3.1) || loss.is_some_and(|l| l > 5.0) || jitter.is_some_and(|j| j > 50.0), "POOR_QUALITY");
    add(finished && !trunk.is_empty() && egress.status >= 400 && !matches!(egress.status, 486 | 487 | 600 | 603), "CARRIER_REJECT");
    add(held, "HOLD");
    add(agent_skewed(app, &capture_ids), "CLOCK_SKEW");
    let ua_l = invite.user_agent.to_ascii_lowercase();
    add(["friendly-scanner", "sipvicious", "sipcli", "pplsip", "vaxsip", "sundayddr"].iter().any(|s| ua_l.contains(s)), "SCANNER");
    let high_risk = app.settings.get("high_risk_prefixes");
    add(high_risk.split(',').map(str::trim).any(|p| !p.is_empty() && callee_n.starts_with(p)), "HIGH_RISK_DESTINATION");

    let ids: Vec<String> = legs.iter().map(|d| d.id.clone()).collect();
    let ts = |v: Option<i64>| v.map(ch_time);
    let row = json!({
        "call_uid": Uuid::new_v5(&NS, root_dialog.id.as_bytes()).to_string(),
        "version": now as u64,
        "state": state,
        "start_ts": ch_time(root.start),
        "ring_ts": ts(ring),
        "answer_ts": ts(answer),
        "end_ts": ts(end_ts),
        "pdd_ms": pdd, "setup_ms": setup, "talk_ms": talk, "egress_pdd_ms": egress_pdd,
        "caller": invite.from_user, "caller_n": caller_n, "callee": invite.ruri_user, "callee_n": callee_n,
        "direction": direction,
        "ingress_group": ingress.map(|g| g.name).unwrap_or_default(),
        "egress_group": egress_group.map(|g| g.name).unwrap_or_default(),
        "trunk": trunk,
        "node": invite.capture_point,
        "pbx": if backend == "asterisk" || backend == "freeswitch" { invite.capture_point.clone() } else { String::new() },
        "backend": backend,
        "capture_ids": capture_ids,
        "capture_points": capture_points,
        "media_node": media_node,
        "final_status": status,
        "final_reason": reason,
        "egress_status": egress.status,
        "q850_cause": q850,
        "end_cause": end_cause,
        "hangup_side": hangup,
        "root_call_id": root_dialog.id,
        "b_call_id": if legs.len() > 1 { legs[1].id.clone() } else { String::new() },
        "legs": ids,
        "link": link,
        "codecs": codecs,
        "ua_caller": invite.user_agent,
        "ua_callee": ua_callee,
        "ua_vendor": vendor, "ua_model": model,
        "caller_ip": invite.src_ip, "callee_ip": egress_invite.dst_ip,
        "caller_country": "",
        "min_mos": mos, "max_loss_pct": loss, "max_jitter_ms": jitter, "max_rtt_ms": rtt,
        "mos_estimated": estimated,
        "media_a_to_b": any_qos, "media_b_to_a": any_qos,
        "findings": f,
        "msg_count": legs.iter().map(|d| d.raw_count).sum::<u32>(),
    });
    (row, recheck)
}

fn id_list(ids: &[String]) -> String {
    ids.iter().map(|i| quote(i)).collect::<Vec<_>>().join(",")
}

fn find(parent: &mut HashMap<String, String>, x: &str) -> String {
    let p = parent.get(x).cloned().unwrap_or_else(|| x.to_string());
    if p == x {
        return p;
    }
    let root = find(parent, &p);
    parent.insert(x.to_string(), root.clone());
    root
}

fn union(parent: &mut HashMap<String, String>, a: &str, b: &str) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        parent.insert(rb, ra);
    }
}

fn digits_tail(s: &str) -> String {
    let d: Vec<char> = s.chars().filter(|c| c.is_ascii_digit()).collect();
    d[d.len().saturating_sub(7)..].iter().collect()
}

async fn fetch(ch: &Ch, ids: &[String]) -> Result<HashMap<String, Vec<M>>, String> {
    let mut out: HashMap<String, Vec<M>> = HashMap::new();
    for chunk in ids.chunks(500) {
        let rows = ch.select(&format!("SELECT {MSG_COLUMNS} FROM sip_messages WHERE call_id IN ({}) ORDER BY ts LIMIT 200000", id_list(chunk))).await?;
        for r in &rows {
            let (id, m) = message(r);
            out.entry(id).or_default().push(m);
        }
    }
    Ok(out)
}

async fn process(app: &App, ch: &Ch, ids: Vec<String>, now: i64, pending: &mut HashMap<String, i64>) -> Result<usize, String> {
    let topo = Topology::parse(&app.settings);
    let mut wanted: BTreeSet<String> = ids.iter().cloned().collect();

    // Legs already joined in an earlier pass stay joined.
    let list = id_list(&ids);
    for r in ch.select(&format!("SELECT legs FROM v_calls WHERE start_ts > now64() - INTERVAL 7 DAY AND hasAny(legs, [{list}])")).await? {
        if let Some(a) = r["legs"].as_array() {
            wanted.extend(a.iter().filter_map(|x| x.as_str().map(str::to_string)));
        }
    }
    let mut messages = fetch(ch, &wanted.iter().cloned().collect::<Vec<_>>()).await?;

    // Explicit links can name a dialog that isn't in this batch.
    let linked: Vec<String> = messages.values().flatten().flat_map(|m| [m.x_cid.clone(), m.hep_cid.clone()]).filter(|c| !c.is_empty() && !wanted.contains(c)).collect::<BTreeSet<_>>().into_iter().collect();
    if !linked.is_empty() {
        messages.extend(fetch(ch, &linked).await?);
    }

    // A PBX that doesn't carry a correlation header can still be matched by number and time.
    let heuristic = app.settings.bool("correlation_heuristic");
    if heuristic && !messages.is_empty() {
        let invites: Vec<&M> = messages.values().filter_map(|m| m.iter().find(|x| x.is_request && x.method == "INVITE" && x.to_tag.is_empty())).collect();
        if let (Some(lo), Some(hi)) = (invites.iter().map(|m| m.ts).min(), invites.iter().map(|m| m.ts).max()) {
            let rows = ch.select(&format!("SELECT DISTINCT call_id FROM sip_messages WHERE ts BETWEEN toDateTime64('{}', 6, 'UTC') AND toDateTime64('{}', 6, 'UTC') AND method = 'INVITE' AND to_tag = ''", ch_time(lo - 4 * SEC), ch_time(hi + 4 * SEC))).await?;
            let extra: Vec<String> = rows.iter().map(|r| s(r, "call_id")).filter(|c| !messages.contains_key(c)).collect();
            if !extra.is_empty() {
                messages.extend(fetch(ch, &extra).await?);
            }
        }
    }

    let dialogs: HashMap<String, Dialog> = messages.into_iter().map(|(id, m)| (id.clone(), Dialog::build(id, m))).filter(|(_, d)| d.first_invite().is_some()).collect();
    let mut parent: HashMap<String, String> = HashMap::new();
    let mut links: HashMap<String, &str> = HashMap::new();
    for d in dialogs.values() {
        for m in &d.msgs {
            for other in [&m.x_cid, &m.hep_cid] {
                if !other.is_empty() && *other != d.id && dialogs.contains_key(other) {
                    union(&mut parent, &d.id, other);
                    links.insert(d.id.clone(), "header");
                    links.insert(other.clone(), "header");
                }
            }
        }
    }
    if heuristic {
        let mut starts: Vec<(i64, &Dialog, &M)> = dialogs.values().filter_map(|d| d.first_invite().map(|i| (i.ts, d, i))).collect();
        starts.sort_by_key(|e| e.0);
        for i in 0..starts.len() {
            let (tb, b, ib) = starts[i];
            if links.contains_key(&b.id) {
                continue;
            }
            // The closest earlier INVITE that arrived at the host this one was sent from.
            let matched = starts[..i].iter().rev().take_while(|(ta, _, _)| tb - ta <= 3 * SEC).find(|(_, a, ia)| {
                a.id != b.id && a.capture_ids.intersection(&b.capture_ids).next().is_some() && ia.dst_ip == ib.src_ip && !digits_tail(&ia.ruri_user).is_empty() && digits_tail(&ia.ruri_user) == digits_tail(&ib.ruri_user)
            });
            if let Some((_, a, _)) = matched {
                union(&mut parent, &a.id, &b.id);
                links.insert(a.id.clone(), "inferred");
                links.insert(b.id.clone(), "inferred");
            }
        }
    }

    let mut groups: HashMap<String, Vec<&Dialog>> = HashMap::new();
    for d in dialogs.values() {
        groups.entry(find(&mut parent, &d.id)).or_default().push(d);
    }

    // Only groups touched by this batch need rewriting.
    let touched: BTreeSet<&String> = wanted.iter().collect();
    let mut groups: Vec<Vec<&Dialog>> = groups.into_values().filter(|g| g.iter().any(|d| touched.contains(&d.id))).collect();
    for g in &mut groups {
        g.sort_by_key(|d| d.first_invite().map(|m| m.ts).unwrap_or(0));
    }

    let leg_ids: Vec<String> = groups.iter().flatten().map(|d| d.id.clone()).collect();
    let mut qos: HashMap<String, Qos> = HashMap::new();
    for chunk in leg_ids.chunks(500) {
        for r in ch.select(&format!("SELECT call_id, min(mos) AS m, max(loss_pct) AS l, max(jitter_ms) AS j, max(rtt_ms) AS r, max(mos_estimated) AS e, any(capture_point) AS n FROM qos_reports WHERE call_id IN ({}) GROUP BY call_id", id_list(chunk))).await? {
            qos.insert(s(&r, "call_id"), Qos {
                min_mos: r["m"].as_f64().map(|x| x as f32),
                loss: r["l"].as_f64().unwrap_or(0.0) as f32,
                jitter: r["j"].as_f64().unwrap_or(0.0) as f32,
                rtt: r["r"].as_f64().map(|x| x as f32),
                estimated: truthy(&r["e"]),
                node: s(&r, "n"),
            });
        }
    }

    let mut body = String::new();
    for g in &groups {
        let link = g.iter().find_map(|d| links.get(&d.id).copied()).unwrap_or("");
        let (row, recheck) = assemble(app, &topo, g, &qos, link, now);
        body.push_str(&row.to_string());
        body.push('\n');
        for d in g {
            match recheck {
                Some(at) => {
                    pending.insert(d.id.clone(), at);
                }
                None => {
                    pending.remove(&d.id);
                }
            }
        }
    }
    if !body.is_empty() {
        ch.insert("calls", body).await?;
    }
    Ok(groups.len())
}

/// After a restart, picks up whatever was still in flight.
async fn recover(app: &App, ch: &Ch) -> Result<(), String> {
    let rows = ch.select("SELECT DISTINCT call_id FROM sip_messages WHERE kind = 'dialog' AND ingest_ts > now64() - INTERVAL 15 MINUTE").await?;
    let open = ch.select("SELECT arrayJoin(legs) AS call_id FROM v_calls WHERE state IN ('setup', 'ringing', 'answered') AND start_ts > now64() - INTERVAL 1 DAY").await?;
    let mut dirty = app.ingest.dirty.lock().unwrap();
    for r in rows.iter().chain(open.iter()) {
        dirty.insert(s(r, "call_id"), 0);
    }
    Ok(())
}

pub async fn run(app: Arc<App>) {
    let mut tick = tokio::time::interval(Duration::from_secs(app.settings.int("finalizer_seconds").max(1) as u64));
    let mut pending: HashMap<String, i64> = HashMap::new();
    let mut recovered_for = String::new();
    loop {
        tick.tick().await;
        let ch = app.storage.read().unwrap().clone();
        let Some(ch) = ch else { continue };
        if recovered_for != ch.url {
            match recover(&app, &ch).await {
                Ok(()) => recovered_for = ch.url.clone(),
                Err(e) => tracing::warn!("finalizer recovery failed: {e}"),
            }
        }
        let now = now_us();
        let ids: Vec<String> = {
            let mut dirty = app.ingest.dirty.lock().unwrap();
            // Give the batch writer time to flush before reading these messages back.
            let ready: Vec<String> = dirty.iter().filter(|(_, t)| now - **t > 2 * SEC).map(|(k, _)| k.clone()).take(BATCH).collect();
            for k in &ready {
                dirty.remove(k);
            }
            ready
        };
        let due: Vec<String> = pending.iter().filter(|(_, at)| **at <= now).map(|(k, _)| k.clone()).collect();
        let mut all = ids;
        for k in due {
            pending.remove(&k);
            all.push(k);
        }
        if all.is_empty() {
            continue;
        }
        if let Err(e) = process(&app, &ch, all.clone(), now, &mut pending).await {
            tracing::warn!("finalizer: {e}");
            let mut dirty = app.ingest.dirty.lock().unwrap();
            for k in all {
                dirty.insert(k, 0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(ts: i64, is_request: bool, method: &str, status: u16, cseq: u32, cm: &str, to_tag: &str) -> M {
        M { ts, capture_id: 1, is_request, method: method.into(), status, reason: "r".into(), cseq, cseq_method: cm.into(), from_tag: "a".into(), to_tag: to_tag.into(), via_branch: format!("b{cseq}{cm}"), src_ip: "1.1.1.1".into(), dst_ip: "2.2.2.2".into(), ..M::default() }
    }

    #[test]
    fn answered_call_ignores_auth_challenge() {
        let d = Dialog::build("x".into(), vec![
            m(0, true, "INVITE", 0, 1, "INVITE", ""),
            m(10, false, "", 407, 1, "INVITE", "t"),
            m(20, true, "ACK", 0, 1, "ACK", "t"),
            m(30, true, "INVITE", 0, 2, "INVITE", ""),
            m(2_000_000, false, "", 180, 2, "INVITE", "t"),
            m(5_000_000, false, "", 200, 2, "INVITE", "t"),
            m(5_000_100, true, "ACK", 0, 2, "ACK", "t"),
            m(9_000_000, true, "BYE", 0, 3, "BYE", "t"),
            m(9_000_100, false, "", 200, 3, "BYE", "t"),
        ]);
        let l = analyze(&d);
        assert_eq!((l.status, l.ring, l.answer), (200, Some(2_000_000), Some(5_000_000)));
        assert_eq!(l.end, Some(9_000_100));
        assert_eq!(l.bye_from_caller, Some(true));
        assert!(!l.ack_missing);
    }

    #[test]
    fn busy_is_final_status() {
        let d = Dialog::build("x".into(), vec![m(0, true, "INVITE", 0, 1, "INVITE", ""), m(100, false, "", 100, 1, "INVITE", ""), m(900, false, "", 486, 1, "INVITE", "t")]);
        let l = analyze(&d);
        assert_eq!((l.status, l.answer, l.end), (486, None, Some(900)));
        assert!(!l.any_provisional, "100 Trying is not progress");
    }

    #[test]
    fn multi_capture_is_not_a_retransmission() {
        let mut second = m(5, true, "INVITE", 0, 1, "INVITE", "");
        second.capture_id = 2;
        let d = Dialog::build("x".into(), vec![m(0, true, "INVITE", 0, 1, "INVITE", ""), second]);
        assert_eq!((d.msgs.len(), d.retrans, d.capture_ids.len()), (1, 0, 2));
    }

    #[test]
    fn detects_missing_ack() {
        let mut msgs = vec![m(0, true, "INVITE", 0, 1, "INVITE", "")];
        for i in 0..5 {
            msgs.push(m(1000 + i, false, "", 200, 1, "INVITE", "t"));
        }
        assert!(analyze(&Dialog::build("x".into(), msgs)).ack_missing);
    }
}
