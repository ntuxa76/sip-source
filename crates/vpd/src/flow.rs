//! Builds a call's ladder diagram from its captured SIP messages.

use crate::ch::Ch;
use crate::model::*;
use crate::sip::{self, SdpInfo};
use crate::sql::quote;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};

#[derive(Clone, Serialize)]
pub struct Column {
    pub name: String,
    pub ip: String,
    pub role: String,
    pub group: String,
    pub capture_id: Option<u32>,
}

#[derive(Clone, Serialize)]
pub struct Capture {
    pub point: String,
    pub capture_id: u32,
    pub ts: Ts,
}

#[derive(Clone, Serialize)]
pub struct DiffRow {
    pub op: &'static str,
    pub name: String,
    pub old: String,
    pub new: String,
}

#[derive(Clone, Serialize)]
pub struct Msg {
    pub idx: usize,
    pub ts: Ts,
    pub leg: &'static str,
    pub call_id: String,
    pub src: usize,
    pub dst: usize,
    pub src_addr: String,
    pub dst_addr: String,
    pub transport: String,
    pub label: String,
    pub method: String,
    pub status: u16,
    pub reason: String,
    pub cseq: u32,
    pub cseq_method: String,
    pub class: &'static str,
    pub retx: u32,
    pub has_sdp: bool,
    pub captures: Vec<Capture>,
    pub hop_of: Option<usize>,
    pub size: u32,
    pub q850: u8,
    pub headers: Vec<(String, String)>,
    pub raw: String,
    pub sdp: Option<SdpInfo>,
    pub diff: Vec<DiffRow>,
}

#[derive(Clone, Serialize, Default)]
pub struct Flow {
    pub columns: Vec<Column>,
    pub messages: Vec<Msg>,
}

pub fn q850_text(c: u8) -> &'static str {
    match c {
        1 => "Unallocated number",
        3 => "No route to destination",
        16 => "Normal clearing",
        17 => "User busy",
        18 => "No user responding",
        19 => "No answer from user",
        20 => "Subscriber absent",
        21 => "Call rejected",
        22 => "Number changed",
        27 => "Destination out of order",
        28 => "Invalid number format",
        31 => "Normal, unspecified",
        34 => "No circuit/channel available",
        38 => "Network out of order",
        41 => "Temporary failure",
        42 => "Switching equipment congestion",
        47 => "Resource unavailable",
        65 => "Bearer capability not implemented",
        88 => "Incompatible destination",
        102 => "Recovery on timer expiry",
        111 => "Protocol error",
        127 => "Interworking, unspecified",
        _ => "",
    }
}

fn hop_diff(prev: &Msg, cur: &Msg) -> Vec<DiffRow> {
    let mut rows = Vec::new();
    let values = |m: &Msg, name: &str| -> Vec<String> { m.headers.iter().filter(|(k, _)| k == name).map(|(_, v)| v.clone()).collect() };
    let mut names: Vec<String> = Vec::new();
    for (k, _) in prev.headers.iter().chain(cur.headers.iter()) {
        if !names.contains(k) {
            names.push(k.clone());
        }
    }
    for name in names {
        let (a, b) = (values(prev, &name), values(cur, &name));
        if a == b {
            continue;
        }
        if a.len() == 1 && b.len() == 1 {
            rows.push(DiffRow { op: "changed", name, old: a[0].clone(), new: b[0].clone() });
            continue;
        }
        for v in b.iter().filter(|v| !a.contains(v)) {
            rows.push(DiffRow { op: "added", name: name.clone(), old: String::new(), new: v.clone() });
        }
        for v in a.iter().filter(|v| !b.contains(v)) {
            rows.push(DiffRow { op: "removed", name: name.clone(), old: v.clone(), new: String::new() });
        }
    }
    if let (Some(a), Some(b)) = (&prev.sdp, &cur.sdp)
        && a.ip != b.ip
    {
        rows.push(DiffRow { op: "changed", name: "SDP c= (media address)".into(), old: a.ip.clone(), new: b.ip.clone() });
    }
    rows
}

fn text(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or("").to_string()
}

fn port(v: &Value, k: &str) -> u64 {
    v[k].as_u64().unwrap_or(0)
}

pub async fn load(ch: &Ch, call: &Call) -> Result<Flow, String> {
    let list = call.legs.iter().map(|l| quote(l)).collect::<Vec<_>>().join(",");
    if list.is_empty() {
        return Ok(Flow::default());
    }
    let rows = ch.select(&format!("SELECT toUnixTimestamp64Micro(ts) AS ts_us, capture_id, capture_point, src_ip, src_port, dst_ip, dst_port, transport, call_id, via_branch, payload FROM sip_messages WHERE call_id IN ({list}) ORDER BY ts LIMIT 10000")).await?;
    Ok(build(&rows, call))
}

fn build(rows: &[Value], call: &Call) -> Flow {
    struct Pending {
        msg: Msg,
        src_ip: String,
        dst_ip: String,
    }
    let mut out: Vec<Pending> = Vec::new();
    let mut hosts: Vec<String> = Vec::new();
    let mut last_capture: HashMap<(String, String, u32, String, String), (usize, u32)> = HashMap::new();
    let mut capture_hosts: HashMap<u32, (String, Option<BTreeSet<String>>)> = HashMap::new();

    for r in rows {
        let raw = text(r, "payload");
        let Some(s) = sip::parse(&raw) else { continue };
        let (src_ip, dst_ip) = (text(r, "src_ip"), text(r, "dst_ip"));
        let capture_id = r["capture_id"].as_u64().unwrap_or(0) as u32;
        let point = text(r, "capture_point");
        let ts = Ts(r["ts_us"].as_i64().unwrap_or(0));
        let src_addr = format!("{src_ip}:{}", port(r, "src_port"));
        let dst_addr = format!("{dst_ip}:{}", port(r, "dst_port"));

        // The agent's own host is the address present in everything it captured.
        let here: BTreeSet<String> = [src_ip.clone(), dst_ip.clone()].into_iter().collect();
        let entry = capture_hosts.entry(capture_id).or_insert_with(|| (point.clone(), None));
        entry.1 = Some(match entry.1.take() {
            None => here,
            Some(set) => set.intersection(&here).cloned().collect(),
        });

        for ip in [&src_ip, &dst_ip] {
            if !hosts.contains(ip) {
                hosts.push(ip.clone());
            }
        }

        let code_or_method = if s.is_request { s.method.clone() } else { s.status.to_string() };
        let key = (format!("{src_addr}>{dst_addr}"), code_or_method, s.cseq_num, s.cseq_method.clone(), text(r, "via_branch"));
        if let Some(&(i, seen_capture)) = last_capture.get(&key) {
            if seen_capture != capture_id && !out[i].msg.captures.iter().any(|c| c.capture_id == capture_id) {
                // The same message seen at another capture point: one arrow with both timestamps.
                out[i].msg.captures.push(Capture { point, capture_id, ts });
                continue;
            }
            if out[i].msg.captures.len() > 1 && seen_capture != capture_id {
                continue;
            }
        }
        let retx = last_capture.get(&key).map(|(i, _)| out[*i].msg.retx + 1).unwrap_or(0);
        let class = match s.status {
            0 => "request",
            100..=199 => "provisional",
            200..=299 => "success",
            300..=399 => "redirect",
            400..=499 => "client_error",
            _ => "server_error",
        };
        let idx = out.len();
        let mut headers = vec![(":start-line".to_string(), s.start_line.clone())];
        headers.extend(s.headers.iter().cloned());
        let leg = if s.call_id == call.root_call_id { "A" } else { "B" };
        let msg = Msg {
            idx,
            ts,
            leg,
            call_id: s.call_id.clone(),
            src: 0,
            dst: 0,
            src_addr,
            dst_addr,
            transport: text(r, "transport"),
            label: if s.is_request { s.method.clone() } else { format!("{} {}", s.status, s.reason) },
            method: s.method.clone(),
            status: s.status,
            reason: s.reason.clone(),
            cseq: s.cseq_num,
            cseq_method: s.cseq_method.clone(),
            class,
            retx,
            has_sdp: s.sdp.is_some(),
            captures: vec![Capture { point, capture_id, ts }],
            hop_of: None,
            size: raw.len() as u32,
            q850: s.q850,
            headers,
            raw,
            sdp: s.sdp.clone(),
            diff: Vec::new(),
        };
        last_capture.insert(key, (idx, capture_id));
        out.push(Pending { msg, src_ip, dst_ip });
    }

    // Columns follow the path: order of first appearance in the signalling.
    let mut named: HashMap<String, (String, u32)> = HashMap::new();
    for (id, (point, set)) in &capture_hosts {
        if let Some(set) = set
            && set.len() == 1
        {
            named.insert(set.iter().next().cloned().unwrap_or_default(), (point.clone(), *id));
        }
    }
    let columns: Vec<Column> = hosts.iter().map(|ip| {
        let agent = named.get(ip);
        Column {
            name: agent.map(|a| a.0.clone()).unwrap_or_else(|| ip.clone()),
            ip: ip.clone(),
            role: if agent.is_some() { "capture".into() } else { "endpoint".into() },
            group: if *ip == call.caller_ip { call.ingress_group.clone() } else if *ip == call.callee_ip { call.egress_group.clone() } else { String::new() },
            capture_id: agent.map(|a| a.1),
        }
    }).collect();

    let mut messages: Vec<Msg> = out.into_iter().map(|p| {
        let mut m = p.msg;
        m.src = hosts.iter().position(|h| *h == p.src_ip).unwrap_or(0);
        m.dst = hosts.iter().position(|h| *h == p.dst_ip).unwrap_or(0);
        m
    }).collect();

    // A proxy hop: the same transaction message forwarded onward by the host that received it.
    for i in 0..messages.len() {
        let prev = (0..i).rev().find(|&j| {
            let (a, b) = (&messages[j], &messages[i]);
            a.call_id == b.call_id && a.cseq == b.cseq && a.cseq_method == b.cseq_method && a.method == b.method && a.status == b.status && a.dst == b.src && a.retx == 0 && a.src != b.dst
        });
        if let Some(j) = prev {
            messages[i].hop_of = Some(j);
            messages[i].diff = hop_diff(&messages[j], &messages[i]);
        }
    }
    Flow { columns, messages }
}
