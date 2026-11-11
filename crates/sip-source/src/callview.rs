//! Per-call detail that isn't stored on the record: QoS streams, log lines, diagnostics, exports.

use crate::ch::Ch;
use crate::flow::{Flow, Msg, q850_text};
use crate::model::*;
use crate::sql::quote;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Serialize)]
pub struct Stream {
    pub id: String,
    pub label: String,
    pub direction: String,
    pub source_type: String,
    pub reporter: String,
    pub ssrc: String,
    pub codec: String,
    pub present: bool,
    pub avg_mos: Option<f32>,
    pub min_mos: Option<f32>,
    pub loss_pct: Option<f32>,
    pub max_jitter_ms: Option<f32>,
    pub avg_rtt_ms: Option<f32>,
    pub verdict: String,
    pub mos_estimated: bool,
    pub points: Vec<(i64, f32, f32, f32, f32)>,
}

fn verdict(mos: f32) -> &'static str {
    match mos {
        m if m >= 4.0 => "good",
        m if m >= 3.6 => "fair",
        m if m >= 3.1 => "poor",
        _ => "bad",
    }
}

fn leg_list(c: &Call) -> String {
    c.legs.iter().map(|l| quote(l)).collect::<Vec<_>>().join(",")
}

pub async fn streams(ch: &Ch, c: &Call) -> Result<Vec<Stream>, String> {
    if c.legs.is_empty() {
        return Ok(vec![]);
    }
    let rows = ch.select(&format!("SELECT toUnixTimestamp64Milli(ts) AS t, ssrc, src_ip, dst_ip, capture_point, ifNull(mos, 0) AS mos, loss_pct, jitter_ms, ifNull(rtt_ms, 0) AS rtt_ms, mos_estimated FROM qos_reports WHERE call_id IN ({}) ORDER BY ts LIMIT 5000", leg_list(c))).await?;
    let mut groups: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for r in &rows {
        groups.entry(r["ssrc"].as_str().unwrap_or("").to_string()).or_default().push(r);
    }
    Ok(groups.into_iter().map(|(ssrc, rs)| {
        let f = |r: &Value, k: &str| r[k].as_f64().unwrap_or(0.0) as f32;
        let points: Vec<(i64, f32, f32, f32, f32)> = rs.iter().map(|r| (r["t"].as_i64().unwrap_or(0), f(r, "mos"), f(r, "loss_pct"), f(r, "jitter_ms"), f(r, "rtt_ms"))).collect();
        let first = rs[0];
        let (src, dst) = (first["src_ip"].as_str().unwrap_or(""), first["dst_ip"].as_str().unwrap_or(""));
        let direction = if src == c.caller_ip || dst == c.callee_ip { "a_to_b" } else { "b_to_a" };
        let n = points.len() as f32;
        let min_mos = points.iter().map(|p| p.1).fold(f32::MAX, f32::min);
        Stream {
            id: ssrc.clone(),
            label: format!("{src} → {dst}"),
            direction: direction.to_string(),
            source_type: "rtcp".into(),
            reporter: first["capture_point"].as_str().unwrap_or("").to_string(),
            ssrc,
            codec: c.codecs.first().cloned().unwrap_or_else(|| "-".into()),
            present: true,
            avg_mos: Some(points.iter().map(|p| p.1).sum::<f32>() / n),
            min_mos: Some(min_mos),
            loss_pct: Some(points.iter().map(|p| p.2).sum::<f32>() / n),
            max_jitter_ms: Some(points.iter().map(|p| p.3).fold(0.0, f32::max)),
            avg_rtt_ms: Some(points.iter().map(|p| p.4).sum::<f32>() / n),
            verdict: verdict(min_mos).to_string(),
            mos_estimated: first["mos_estimated"].as_bool().unwrap_or(first["mos_estimated"].as_u64().unwrap_or(0) != 0),
            points,
        }
    }).collect())
}

pub async fn logs(ch: &Ch, c: &Call) -> Result<Value, String> {
    if c.legs.is_empty() {
        return Ok(json!([]));
    }
    let rows = ch.select(&format!("SELECT toUnixTimestamp64Micro(ts) AS t, node, severity, message FROM logs WHERE call_id IN ({}) ORDER BY ts LIMIT 2000", leg_list(c))).await?;
    Ok(json!(rows.iter().map(|r| json!({"ts": Ts(r["t"].as_i64().unwrap_or(0)), "node": r["node"], "severity": r["severity"], "message": r["message"]})).collect::<Vec<_>>()))
}

fn finding_text(code: &str) -> (&'static str, &'static str, &'static str, &'static str) {
    match code {
        "NO_FINAL_RESPONSE" => ("critical", "INVITE never got a final response", "The INVITE was retransmitted until Timer B expired and no final response came back from the next hop.", "Check reachability of the next hop and whether its SIP port answers at the time of the call."),
        "ACK_MISSING" => ("critical", "200 OK was never acknowledged", "The 200 OK to the INVITE kept being retransmitted because the ACK never arrived, and the call was torn down about 32 s after answer.", "Usually NAT or a wrong Contact/Record-Route. Compare the Contact in the 200 OK with where the ACK should be sent."),
        "DROP_AFTER_32S" => ("warning", "Call dropped about 32 s after answer", "The answered call ended between 30 and 33 seconds after answer, which is the signature of a lost ACK or a NAT problem.", "Look at the ACK path on the A leg, and at SIP ALG on the customer router."),
        "AUTH_LOOP" => ("warning", "Repeated authentication challenges", "Three or more consecutive challenges were answered without success, so credentials were rejected.", "Check the password in the phone's provisioning profile, then reset the SIP credentials."),
        "NO_COMMON_CODEC" => ("warning", "No codec in common", "The far end refused the session with 488 or 606.", "Compare the offered codecs with what the far end allows, and enable a common codec such as PCMA."),
        "PRIVATE_IP_IN_SDP" => ("warning", "Private address in SDP", "The SDP connection address is private while signalling comes from a public address, so the media path depends on NAT handling.", "Confirm the call is anchored on a media relay, and fix NAT settings on the device."),
        "HIGH_PDD" => ("warning", "High post-dial delay", "The caller waited longer than the configured threshold between the INVITE and the first ringing or final response.", "Compare the egress PDD: if it is also high, the delay is on the far side."),
        "RETRANSMISSIONS" => ("warning", "Request retransmitted", "A request was retransmitted three or more times on one hop, which points to packet loss or a slow next hop.", "Check loss between these two hosts and the load on the receiving node."),
        "POOR_QUALITY" => ("warning", "Poor media quality", "Worst MOS was below 3.1, or loss above 5 %, or jitter above 50 ms.", "Open the Media tab to find the worst window, and check the node and access network for that stream."),
        "CARRIER_REJECT" => ("info", "Rejected by the carrier", "The egress leg was rejected by the carrier. The status code and Q.850 cause come from the carrier.", "If many calls show the same code, check the trunk's capacity and raise it with the carrier."),
        "CLOCK_SKEW" => ("info", "Capture times disagree", "A capture agent's clock is off by more than a second, so message order in the flow may be slightly wrong.", "Check NTP on the affected agent."),
        "HOLD" => ("info", "Call was placed on hold", "Hold re-INVITEs were seen during the call.", "No action needed unless the customer reports a problem at this point in the call."),
        "SCANNER" => ("warning", "Known scanner", "The request came from a known scanner user agent.", "Confirm that scanners are blocked at the edge."),
        "HIGH_RISK_DESTINATION" => ("warning", "High-risk destination", "The called number matches the high-risk prefix list, which is a toll-fraud indicator.", "Check the account's recent call volume and whether the customer expects these destinations."),
        _ => ("info", "Finding", "", ""),
    }
}

fn evidence(code: &str, msgs: &[Msg]) -> Vec<usize> {
    let pick = |f: &dyn Fn(&Msg) -> bool| -> Vec<usize> { msgs.iter().filter(|m| f(m)).map(|m| m.idx).take(6).collect() };
    match code {
        "ACK_MISSING" | "NO_FINAL_RESPONSE" | "RETRANSMISSIONS" => pick(&|m| m.retx > 0),
        "AUTH_LOOP" => pick(&|m| matches!(m.status, 401 | 403 | 407)),
        "CARRIER_REJECT" | "NO_COMMON_CODEC" => pick(&|m| m.status >= 400),
        "PRIVATE_IP_IN_SDP" => pick(&|m| m.sdp.is_some()),
        "HIGH_PDD" => pick(&|m| m.status == 180 || m.status == 183).into_iter().take(2).collect(),
        "HOLD" => pick(&|m| m.method == "INVITE" && m.has_sdp && m.cseq > 2),
        "DROP_AFTER_32S" => pick(&|m| m.method == "BYE"),
        "SCANNER" => pick(&|m| m.method == "INVITE"),
        _ => vec![],
    }
}

pub fn findings(c: &Call, flow: &Flow) -> Value {
    let rank = |s: &str| match s {
        "critical" => 0,
        "warning" => 1,
        _ => 2,
    };
    let mut items: Vec<Value> = c.findings.iter().map(|code| {
        let (severity, title, explanation, next) = finding_text(code);
        let mut measured = Vec::new();
        match code.as_str() {
            "HIGH_PDD" => measured.push(format!("PDD {} ms", c.pdd_ms.unwrap_or(0))),
            "POOR_QUALITY" => measured.push(format!("MOS {:.2}, loss {:.1} %", c.min_mos.unwrap_or(0.0), c.max_loss_pct.unwrap_or(0.0))),
            "CARRIER_REJECT" => measured.push(format!("{} {} · Q.850 {} {}", c.egress_status, c.final_reason, c.q850_cause, q850_text(c.q850_cause))),
            "DROP_AFTER_32S" => measured.push(format!("Talk time {:.1} s", c.talk_ms as f64 / 1000.0)),
            _ => {}
        }
        json!({"code": code, "severity": severity, "title": title, "explanation": explanation, "next_step": next, "evidence": evidence(code, &flow.messages), "measured": measured})
    }).collect();
    items.sort_by_key(|v| rank(v["severity"].as_str().unwrap_or("")));
    Value::Array(items)
}

pub fn severity_of(code: &str) -> &'static str {
    finding_text(code).0
}

/// sngrep-style text: the flow, then the full messages.
pub fn text_export(c: &Call, flow: &Flow) -> String {
    let mut out = format!("Call {}\n{} -> {}  {} {}\nA-leg {}\n\n", c.call_uid, c.caller, c.callee, c.final_status, c.final_reason, c.root_call_id);
    for m in &flow.messages {
        let dir = format!("{} -> {}", flow.columns[m.src].name, flow.columns[m.dst].name);
        out.push_str(&format!("{:>4}  {:<46} {:<34} CSeq {} {}\n", m.idx + 1, dir, m.label, m.cseq, m.cseq_method));
    }
    for m in &flow.messages {
        out.push_str(&format!("\n--- #{} {} -> {} ---\n{}\n", m.idx + 1, m.src_addr, m.dst_addr, m.raw.replace('\r', "")));
    }
    out
}

/// Classic libpcap file holding the call's messages as UDP/IPv4 frames.
pub fn pcap_export(flow: &Flow) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0xa1b2c3d4u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 8]);
    out.extend_from_slice(&65535u32.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    let parse = |a: &str| -> ([u8; 4], u16) {
        let (ip, port) = a.rsplit_once(':').unwrap_or((a, "5060"));
        let mut b = [0u8; 4];
        for (i, p) in ip.split('.').take(4).enumerate() {
            b[i] = p.parse().unwrap_or(0);
        }
        (b, port.parse().unwrap_or(5060))
    };
    for m in &flow.messages {
        let (src, sport) = parse(&m.src_addr);
        let (dst, dport) = parse(&m.dst_addr);
        let payload = m.raw.as_bytes();
        let udp_len = 8 + payload.len() as u16;
        let ip_len = 20 + udp_len;
        let mut frame = Vec::with_capacity(14 + ip_len as usize);
        frame.extend_from_slice(&[0x02, 0, 0, 0, 0, 2, 0x02, 0, 0, 0, 0, 1, 0x08, 0x00]);
        let mut ip = vec![0x45, 0];
        ip.extend_from_slice(&ip_len.to_be_bytes());
        ip.extend_from_slice(&[0, 0, 0x40, 0, 64, 17, 0, 0]);
        ip.extend_from_slice(&src);
        ip.extend_from_slice(&dst);
        let mut sum: u32 = ip.chunks(2).map(|w| u16::from_be_bytes([w[0], w[1]]) as u32).sum();
        while sum > 0xffff {
            sum = (sum & 0xffff) + (sum >> 16);
        }
        ip[10..12].copy_from_slice(&(!(sum as u16)).to_be_bytes());
        frame.extend_from_slice(&ip);
        frame.extend_from_slice(&sport.to_be_bytes());
        frame.extend_from_slice(&dport.to_be_bytes());
        frame.extend_from_slice(&udp_len.to_be_bytes());
        frame.extend_from_slice(&[0, 0]);
        frame.extend_from_slice(payload);
        out.extend_from_slice(&((m.ts.0 / SEC) as u32).to_le_bytes());
        out.extend_from_slice(&((m.ts.0 % SEC) as u32).to_le_bytes());
        out.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        out.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        out.extend_from_slice(&frame);
    }
    out
}
