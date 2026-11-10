//! The capture pipeline: HEP listeners, decoding, batching into ClickHouse and per-agent statistics.
//! Receiving never waits on storage: when the queue is full a packet is dropped and counted.

use crate::api::{App, now_us};
use crate::hep::{self, Hep};
use crate::model::*;
use crate::search::normalize;
use crate::sip;
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::Row;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::{broadcast, mpsc};

const QUEUE: usize = 200_000;
const FLUSH_ROWS: usize = 20_000;

enum Row_ {
    Sip(String),
    Registration(String),
    Qos(String),
    Log(String),
}

/// One HEP sender, identified by its address. Several servers often share Asterisk's default
/// capture ID, so the address is what tells them apart.
#[derive(Default, Clone, Serialize)]
pub struct AgentStats {
    pub ip: String,
    pub alias: String,
    pub capture_id: u32,
    pub hep_version: u8,
    pub packets: u64,
    pub bytes: u64,
    pub errors: u64,
    pub last_seen: Option<Ts>,
    pub first_seen: Option<Ts>,
    pub clock_offset_ms: f64,
    #[serde(skip)]
    minute_packets: u64,
    #[serde(skip)]
    minute_offset_sum: f64,
}

impl AgentStats {
    /// The alias given in Admin, or the address.
    pub fn label(&self) -> String {
        if self.alias.is_empty() { self.ip.clone() } else { self.alias.clone() }
    }
}

struct PendingRegister {
    ts: i64,
    sip: sip::Sip,
    src_ip: String,
    src_port: u16,
    transport: String,
    capture_id: u32,
    agent_ip: String,
    node: String,
    challenged: bool,
}

#[derive(Default)]
struct Minute {
    methods: HashMap<String, u64>,
    classes: HashMap<String, u64>,
    nodes: HashMap<String, u64>,
    drops: HashMap<String, u64>,
    lag_sum_ms: f64,
    lag_n: u64,
    insert_ms_sum: f64,
    insert_n: u64,
}

pub struct Ingest {
    tx: mpsc::Sender<Row_>,
    /// Keyed by the sender's address.
    pub agents: RwLock<HashMap<String, AgentStats>>,
    /// Call-IDs with new messages, and when they were last touched.
    pub dirty: Mutex<HashMap<String, i64>>,
    registers: Mutex<HashMap<(String, u32), PendingRegister>>,
    minute: Mutex<Minute>,
    pub totals: Mutex<HashMap<String, u64>>,
    /// Live capture only pays for serialization while someone is subscribed.
    pub live: broadcast::Sender<Arc<String>>,
}

fn count(map: &mut HashMap<String, u64>, key: &str) {
    *map.entry(key.to_string()).or_default() += 1;
}

impl Ingest {
    fn drop_packet(&self, reason: &str) {
        count(&mut self.minute.lock().unwrap().drops, reason);
        count(&mut self.totals.lock().unwrap(), reason);
    }

    fn send(&self, row: Row_) {
        if self.tx.try_send(row).is_err() {
            self.drop_packet("overload");
        }
    }

    fn agent_name(&self, h: &Hep, peer: &str, offset_ms: f64, bytes: usize) -> (String, bool) {
        let now = Ts(now_us());
        let mut agents = self.agents.write().unwrap();
        let new = !agents.contains_key(peer);
        let a = agents.entry(peer.to_string()).or_insert_with(|| AgentStats { ip: peer.to_string(), first_seen: Some(now), ..AgentStats::default() });
        // A name sent in HEP is a starting alias; one set in Admin is never replaced.
        if let Some(n) = &h.agent_name
            && a.alias.is_empty()
        {
            a.alias = n.clone();
        }
        a.capture_id = h.capture_id;
        a.hep_version = h.version;
        a.packets += 1;
        a.bytes += bytes as u64;
        a.last_seen = Some(now);
        a.minute_packets += 1;
        a.minute_offset_sum += offset_ms;
        (a.label(), new)
    }
}

pub struct RowBox(mpsc::Receiver<Row_>);

pub fn new() -> (Ingest, RowBox) {
    let (tx, rx) = mpsc::channel(QUEUE);
    let (live, _) = broadcast::channel(4096);
    (Ingest {
        tx,
        agents: RwLock::new(HashMap::new()),
        dirty: Mutex::new(HashMap::new()),
        registers: Mutex::new(HashMap::new()),
        minute: Mutex::new(Minute::default()),
        totals: Mutex::new(HashMap::new()),
        live,
    }, RowBox(rx))
}

fn is_private(ip: &str) -> bool {
    let p: Vec<u32> = ip.split('.').filter_map(|x| x.parse().ok()).collect();
    p.len() == 4 && (p[0] == 10 || (p[0] == 172 && (16..=31).contains(&p[1])) || (p[0] == 192 && p[1] == 168) || (p[0] == 100 && (64..=127).contains(&p[1])))
}

fn transport(h: &Hep, via: &str) -> String {
    match via {
        "ws" | "wss" | "tls" => via.to_string(),
        _ => match h.ip_proto {
            6 => "tcp".into(),
            132 => "sctp".into(),
            _ => "udp".into(),
        },
    }
}

fn handle_sip(app: &App, ing: &Ingest, h: &Hep, ts: i64, ingest_ts: i64, node: &str, peer: &str) {
    let text = String::from_utf8_lossy(&h.payload);
    let Some(s) = sip::parse(&text) else {
        ing.drop_packet("parse_error");
        let row = json!({
            "ts": ch_time(ts), "ingest_ts": ch_time(ingest_ts), "kind": "other", "capture_id": h.capture_id, "capture_point": node, "agent_ip": peer,
            "src_ip": h.src_ip, "src_port": h.src_port, "dst_ip": h.dst_ip, "dst_port": h.dst_port, "transport": "udp",
            "call_id": "", "parse_error": true, "payload": text.as_ref(),
        });
        ing.send(Row_::Sip(row.to_string()));
        return;
    };
    let is_register = s.cseq_method == "REGISTER";
    let is_dialog = matches!(s.cseq_method.as_str(), "INVITE" | "ACK" | "BYE" | "CANCEL" | "UPDATE" | "PRACK" | "REFER" | "INFO");
    let x_cid = app.settings.get("correlation_headers").split(',').map(str::trim).find_map(|name| sip::header(&s, name).map(str::to_string)).unwrap_or_default();
    let (codecs, sdp_ip, sdp_dir) = s.sdp.as_ref().map(|d| (d.codecs.clone(), d.ip.clone(), d.direction.clone())).unwrap_or_default();
    let row = json!({
        "ts": ch_time(ts), "ingest_ts": ch_time(ingest_ts), "kind": if is_dialog { "dialog" } else { "other" },
        "capture_id": h.capture_id, "capture_point": node, "agent_ip": peer,
        "src_ip": h.src_ip, "src_port": h.src_port, "dst_ip": h.dst_ip, "dst_port": h.dst_port,
        "transport": transport(h, &s.via_transport), "call_id": s.call_id, "is_request": s.is_request, "method": s.method,
        "status": s.status, "reason": s.reason, "cseq_num": s.cseq_num, "cseq_method": s.cseq_method,
        "from_user": s.from_user, "from_user_n": normalize(&s.from_user), "from_domain": s.from_domain, "from_tag": s.from_tag,
        "to_user": s.to_user, "to_user_n": normalize(&s.to_user), "to_domain": s.to_domain, "to_tag": s.to_tag,
        "ruri_user": s.ruri_user, "ruri_user_n": normalize(&s.ruri_user), "pai_user_n": normalize(&s.pai_user),
        "user_agent": s.user_agent, "via_branch": s.via_branch, "has_sdp": s.sdp.is_some(), "codecs": codecs, "sdp_ip": sdp_ip, "sdp_dir": sdp_dir,
        "q850_cause": s.q850, "x_cid": x_cid, "hep_cid": h.correlation.clone().unwrap_or_default(), "parse_error": false,
        "payload": sip::mask_crypto(&text),
    });
    ing.send(Row_::Sip(row.to_string()));

    {
        let mut m = ing.minute.lock().unwrap();
        count(&mut m.nodes, node);
        if s.is_request {
            count(&mut m.methods, &s.method);
        } else {
            count(&mut m.classes, &format!("{}xx", s.status / 100));
        }
    }
    if is_dialog {
        ing.dirty.lock().unwrap().insert(s.call_id.clone(), ingest_ts);
    }
    if is_register {
        handle_register(app, ing, h, ts, node, peer, s.clone());
    }
    if ing.live.receiver_count() > 0 {
        let event = json!({
            "ts": Ts(ts), "call_id": s.call_id, "label": if s.is_request { s.method.clone() } else { format!("{} {}", s.status, s.reason) },
            "class": match s.status { 0 => "request", 100..=199 => "provisional", 200..=299 => "success", 300..=399 => "redirect", 400..=499 => "client_error", _ => "server_error" },
            "from": format!("{}:{}", h.src_ip, h.src_port), "to": format!("{}:{}", h.dst_ip, h.dst_port), "retx": 0,
            "caller": s.from_user, "callee": s.to_user, "state": if s.is_request { s.method.to_lowercase() } else { s.status.to_string() },
            "node": node, "ua": s.user_agent, "capture_id": h.capture_id, "agent_ip": peer, "cseq_method": s.cseq_method,
        });
        let _ = ing.live.send(Arc::new(event.to_string()));
    }
}

fn group_of(app: &App, ip: &str) -> String {
    crate::finalizer::Topology::parse(&app.settings).classify(ip).map(|g| g.name).unwrap_or_default()
}

fn write_registration(app: &App, ing: &Ingest, p: &PendingRegister, result: &str, code: u16, response: Option<&sip::Sip>, end_ts: Option<i64>) {
    let expires_requested = p.sip.contact_expires.or(p.sip.expires).unwrap_or(3600);
    let granted = response.and_then(|r| r.contact_expires.or(r.expires)).unwrap_or(if code / 100 == 2 { expires_requested } else { 0 });
    let number = normalize(&p.sip.to_user);
    let contact_ip = sip::uri_parts(&p.sip.contact).1.split(':').next().unwrap_or("").to_string();
    let row = json!({
        "ts": ch_time(p.ts), "aor": format!("{}@{}", p.sip.to_user, p.sip.to_domain), "number": number, "grp": group_of(app, &p.src_ip),
        "contact": p.sip.contact, "src_ip": p.src_ip, "src_port": p.src_port, "transport": p.transport, "user_agent": p.sip.user_agent,
        "expires_requested": expires_requested, "expires_granted": granted, "result": result, "code": code,
        "rrd_ms": end_ts.map(|e| ((e - p.ts) / 1000).max(0)), "node": p.node, "capture_id": p.capture_id, "agent_ip": p.agent_ip,
        "nat": !contact_ip.is_empty() && contact_ip != p.src_ip, "challenged": p.challenged,
    });
    ing.send(Row_::Registration(row.to_string()));
}

fn handle_register(app: &App, ing: &Ingest, h: &Hep, ts: i64, node: &str, peer: &str, s: sip::Sip) {
    let key = (s.call_id.clone(), s.cseq_num);
    let mut map = ing.registers.lock().unwrap();
    if s.is_request {
        // A retransmission keeps the first attempt's timing.
        if map.contains_key(&key) {
            return;
        }
        let challenged = map.iter().any(|((cid, seq), p)| *cid == s.call_id && *seq < s.cseq_num && p.challenged);
        map.insert(key, PendingRegister { ts, transport: transport(h, &s.via_transport), sip: s, src_ip: h.src_ip.clone(), src_port: h.src_port, capture_id: h.capture_id, agent_ip: peer.to_string(), node: node.to_string(), challenged });
        return;
    }
    if s.status < 200 {
        return;
    }
    let Some(mut p) = map.remove(&key) else { return };
    if s.status == 401 || s.status == 407 {
        // A challenge isn't an outcome unless no authenticated retry follows, which the sweep handles.
        p.challenged = true;
        p.ts = ts;
        map.insert((s.call_id.clone(), s.cseq_num), p);
        return;
    }
    map.retain(|(cid, _), _| *cid != s.call_id);
    drop(map);
    let granted_zero = s.contact_expires.or(s.expires) == Some(0) || p.sip.contact.is_empty();
    let result = match s.status {
        200..=299 if granted_zero => "deregistered",
        200..=299 if p.challenged => "registered",
        200..=299 => "refreshed",
        403 => "auth_failed",
        _ => "rejected",
    };
    write_registration(app, ing, &p, result, s.status, Some(&s), Some(ts));
}

/// Registrations that never got a final answer, or whose challenge was never answered.
fn sweep_registers(app: &App, ing: &Ingest) {
    let now = now_us();
    let mut expired = Vec::new();
    {
        let mut map = ing.registers.lock().unwrap();
        let keys: Vec<(String, u32)> = map.iter().filter(|(_, p)| now - p.ts > 32 * SEC).map(|(k, _)| k.clone()).collect();
        for k in keys {
            if let Some(p) = map.remove(&k) {
                expired.push(p);
            }
        }
    }
    for p in expired {
        if p.challenged {
            write_registration(app, ing, &p, "auth_failed", 401, None, None);
        } else {
            write_registration(app, ing, &p, "no_response", 408, None, None);
        }
    }
}

fn estimate_mos(loss_pct: f64, jitter_ms: f64, rtt_ms: f64) -> f64 {
    // A simplified ITU-T G.107 E-model.
    let delay = rtt_ms / 2.0 + jitter_ms * 2.0 + 10.0;
    let id = 0.024 * delay + if delay > 177.3 { 0.11 * (delay - 177.3) } else { 0.0 };
    let ie = 30.0 * (1.0 + 0.15 * loss_pct).ln();
    let r = (93.2 - id - ie).clamp(0.0, 100.0);
    (1.0 + 0.035 * r + r * (r - 60.0) * (100.0 - r) * 7e-6).clamp(1.0, 4.5)
}

/// Normalizes the RTCP JSON sent by Asterisk (res_hep_rtcp) and rtpengine.
fn handle_rtcp(ing: &Ingest, h: &Hep, ts: i64, node: &str, peer: &str) {
    let Ok(v) = serde_json::from_slice::<Value>(&h.payload) else {
        ing.drop_packet("bad_rtcp");
        return;
    };
    let Some(call_id) = h.correlation.clone() else {
        ing.drop_packet("rtcp_without_call_id");
        return;
    };
    let block = v["report_blocks"].as_array().and_then(|a| a.first());
    let fraction = block.and_then(|b| b["fraction_lost"].as_f64()).unwrap_or(0.0);
    let loss_pct = fraction * 100.0 / 256.0;
    let rate = v["sample_rate"].as_f64().unwrap_or(8000.0);
    let jitter_ms = block.and_then(|b| b["ia_jitter"].as_f64()).map(|j| j * 1000.0 / rate).unwrap_or(0.0);
    let rtt_ms = v["rtt"].as_f64().or(v["RTT"].as_f64()).map(|r| if r < 10.0 { r * 1000.0 } else { r });
    let packets = v["sender_information"]["packets"].as_u64().unwrap_or(0);
    let lost = block.and_then(|b| b["packets_lost"].as_i64()).unwrap_or(0);
    let reported = v["mos"].as_f64().or(v["MOS"].as_f64()).filter(|m| *m > 0.0);
    // A sender report with no report block says nothing about received quality.
    let mos = reported.or_else(|| block.map(|_| estimate_mos(loss_pct, jitter_ms, rtt_ms.unwrap_or(0.0))));
    let row = json!({
        "ts": ch_time(ts), "call_id": call_id, "capture_id": h.capture_id, "capture_point": node, "agent_ip": peer, "source": "rtcp",
        "ssrc": v["ssrc"].to_string().trim_matches('"'), "src_ip": h.src_ip, "dst_ip": h.dst_ip, "packets": packets, "lost": lost,
        "loss_pct": loss_pct, "jitter_ms": jitter_ms, "rtt_ms": rtt_ms, "mos": mos, "mos_estimated": reported.is_none() && mos.is_some(),
    });
    ing.send(Row_::Qos(row.to_string()));
    ing.dirty.lock().unwrap().insert(call_id, now_us());
}

fn handle_log(ing: &Ingest, h: &Hep, ts: i64, node: &str, peer: &str) {
    let text = String::from_utf8_lossy(&h.payload).to_string();
    let parsed: Option<Value> = serde_json::from_str(&text).ok();
    let severity = parsed.as_ref().and_then(|v| v["level"].as_str().or(v["severity"].as_str()).map(str::to_uppercase)).unwrap_or_else(|| {
        ["ERROR", "WARNING", "NOTICE", "DEBUG"].iter().find(|s| text.contains(**s)).unwrap_or(&"INFO").to_string()
    });
    let message = parsed.as_ref().and_then(|v| v["message"].as_str().or(v["msg"].as_str()).map(str::to_string)).unwrap_or(text);
    let row = json!({"ts": ch_time(ts), "call_id": h.correlation.clone().unwrap_or_default(), "capture_id": h.capture_id, "agent_ip": peer, "node": node, "severity": severity, "message": message});
    ing.send(Row_::Log(row.to_string()));
}

fn handle_packet(app: &App, data: &[u8], peer: &str) {
    let ing = &app.ingest;
    let h = match hep::decode(data) {
        Ok(h) => h,
        Err(reason) => {
            ing.drop_packet(reason);
            return;
        }
    };
    let key = app.settings.get("hep_auth_key");
    if !key.is_empty() && h.auth_key.as_deref() != Some(key.as_str()) {
        ing.drop_packet("auth");
        return;
    }
    let ingest_ts = now_us();
    let ts = if h.ts_sec > 0 { h.ts_sec as i64 * SEC + h.ts_usec as i64 } else { ingest_ts };
    let offset_ms = (ingest_ts - ts) as f64 / 1000.0;
    let (node, new_agent) = ing.agent_name(&h, peer, offset_ms, data.len());
    if new_agent {
        tracing::info!("new HEP source {peer} (capture ID {})", h.capture_id);
    }
    {
        let mut m = ing.minute.lock().unwrap();
        m.lag_sum_ms += offset_ms.max(0.0);
        m.lag_n += 1;
    }
    match h.proto_type {
        1 => handle_sip(app, ing, &h, ts, ingest_ts, &node, peer),
        5 => handle_rtcp(ing, &h, ts, &node, peer),
        100 => handle_log(ing, &h, ts, &node, peer),
        other => ing.drop_packet(&format!("unsupported_type_{other}")),
    }
}

async fn udp_listener(app: Arc<App>, addr: String) -> Result<(), String> {
    let socket = UdpSocket::bind(&addr).await.map_err(|e| format!("can't listen for HEP on udp {addr}: {e}"))?;
    tracing::info!("listening for HEP on udp {addr}");
    tokio::spawn(async move {
        let mut buf = vec![0u8; 65_536];
        loop {
            match socket.recv_from(&mut buf).await {
                Ok((n, peer)) => handle_packet(&app, &buf[..n], &peer.ip().to_string()),
                Err(e) => tracing::warn!("HEP receive error: {e}"),
            }
        }
    });
    Ok(())
}

async fn tcp_listener(app: Arc<App>, addr: String) -> Result<(), String> {
    let listener = TcpListener::bind(&addr).await.map_err(|e| format!("can't listen for HEP on tcp {addr}: {e}"))?;
    tracing::info!("listening for HEP on tcp {addr}");
    tokio::spawn(async move {
        loop {
            let Ok((stream, peer)) = listener.accept().await else { continue };
            let app = app.clone();
            tokio::spawn(tcp_connection(app, stream, peer));
        }
    });
    Ok(())
}

async fn tcp_connection(app: Arc<App>, mut stream: tokio::net::TcpStream, peer: SocketAddr) {
    let ip = peer.ip().to_string();
    let mut buf: Vec<u8> = Vec::with_capacity(65_536);
    let mut chunk = [0u8; 16_384];
    loop {
        match tokio::time::timeout(Duration::from_secs(300), stream.read(&mut chunk)).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => return,
            Ok(Ok(n)) => buf.extend_from_slice(&chunk[..n]),
        }
        loop {
            match hep::frame_len(&buf) {
                Ok(Some(len)) => {
                    handle_packet(&app, &buf[..len], &ip);
                    buf.drain(..len);
                }
                Ok(None) => break,
                Err(reason) => {
                    app.ingest.drop_packet(reason);
                    return;
                }
            }
        }
    }
}

async fn flush(app: &App, sip: &mut String, reg: &mut String, qos: &mut String, log: &mut String, counts: &mut [u64; 4]) {
    let ch = app.storage.read().unwrap().clone();
    for (table, rows, i) in [("sip_messages", &mut *sip, 0usize), ("registration_events", &mut *reg, 1), ("qos_reports", &mut *qos, 2), ("logs", &mut *log, 3)] {
        if rows.is_empty() {
            continue;
        }
        // While the selected storage starts, hold rows for a while instead of dropping them.
        if ch.is_none() && app.storage_starting.load(std::sync::atomic::Ordering::SeqCst) && rows.len() < 64 << 20 {
            continue;
        }
        let body = std::mem::take(rows);
        let Some(ch) = &ch else {
            app.ingest.drop_packet("no_storage");
            counts[i] = 0;
            continue;
        };
        let started = std::time::Instant::now();
        let mut ok = false;
        for attempt in 0..3 {
            match ch.insert(table, body.clone()).await {
                Ok(()) => {
                    ok = true;
                    break;
                }
                Err(e) => {
                    tracing::warn!("insert into {table} failed (attempt {}): {e}", attempt + 1);
                    tokio::time::sleep(Duration::from_millis(300 * (attempt + 1))).await;
                }
            }
        }
        if ok {
            let mut m = app.ingest.minute.lock().unwrap();
            m.insert_ms_sum += started.elapsed().as_secs_f64() * 1000.0;
            m.insert_n += 1;
        } else {
            tracing::error!("dropped {} rows for {table}: ClickHouse insert failed", counts[i]);
            for _ in 0..counts[i] {
                app.ingest.drop_packet("insert_failed");
            }
        }
        counts[i] = 0;
    }
}

async fn writer(app: Arc<App>, mut rx: mpsc::Receiver<Row_>) {
    let (mut sip, mut reg, mut qos, mut log) = (String::new(), String::new(), String::new(), String::new());
    let mut counts = [0u64; 4];
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            row = rx.recv() => {
                let Some(row) = row else { return };
                let (buf, i, line) = match row {
                    Row_::Sip(l) => (&mut sip, 0, l),
                    Row_::Registration(l) => (&mut reg, 1, l),
                    Row_::Qos(l) => (&mut qos, 2, l),
                    Row_::Log(l) => (&mut log, 3, l),
                };
                buf.push_str(&line);
                buf.push('\n');
                counts[i] += 1;
                if counts.iter().sum::<u64>() as usize >= FLUSH_ROWS {
                    flush(&app, &mut sip, &mut reg, &mut qos, &mut log, &mut counts).await;
                }
            }
            _ = tick.tick() => flush(&app, &mut sip, &mut reg, &mut qos, &mut log, &mut counts).await,
        }
    }
}

/// Once a minute: capture health counters and the count of active registrations.
async fn minute_job(app: Arc<App>) {
    let mut tick = tokio::time::interval(Duration::from_secs(60));
    tick.tick().await;
    loop {
        tick.tick().await;
        sweep_registers(&app, &app.ingest);
        let bucket = (now_us() - MIN) / MIN * MIN / SEC;
        let time = chrono::DateTime::from_timestamp(bucket, 0).unwrap_or_default().format("%Y-%m-%d %H:%M:%S").to_string();
        let mut out = String::new();
        let mut push = |metric: &str, dim: &str, name: &str, value: f64| {
            out.push_str(&json!({"bucket": time, "metric": metric, "dim": dim, "name": name, "value": value}).to_string());
            out.push('\n');
        };
        let ips: Vec<String>;
        {
            let mut agents = app.ingest.agents.write().unwrap();
            ips = agents.keys().cloned().collect();
            for a in agents.values_mut() {
                push("hep_pps", "default", &a.label(), a.minute_packets as f64 / 60.0);
                if a.minute_packets > 0 {
                    a.clock_offset_ms = a.minute_offset_sum / a.minute_packets as f64;
                    push("clock_offset", "default", &a.label(), a.clock_offset_ms);
                }
                a.minute_packets = 0;
                a.minute_offset_sum = 0.0;
            }
        }
        {
            let mut m = app.ingest.minute.lock().unwrap();
            for (k, v) in m.methods.drain() {
                push("sip_msgs", "method", &k, v as f64 / 60.0);
            }
            for (k, v) in m.classes.drain() {
                push("sip_msgs", "status_class", &k, v as f64 / 60.0);
            }
            for (k, v) in m.nodes.drain() {
                push("sip_msgs", "node", &k, v as f64 / 60.0);
            }
            for (k, v) in m.drops.drain() {
                push("hep_drops", "default", &k, v as f64 / 60.0);
            }
            if m.lag_n > 0 {
                push("ingest_lag", "default", "value", m.lag_sum_ms / m.lag_n as f64);
            }
            if m.insert_n > 0 {
                push("ch_insert", "default", "value", m.insert_ms_sum / m.insert_n as f64);
            }
            *m = Minute::default();
        }
        persist_sources(&app, &ips).await;
        let ch = app.storage.read().unwrap().clone();
        let Some(ch) = ch else { continue };
        if !out.is_empty()
            && let Err(e) = ch.insert("system_1m", out).await
        {
            tracing::warn!("could not write capture metrics: {e}");
        }
        let sql = format!("INSERT INTO reg_active_1m SELECT toDateTime('{time}', 'UTC') AS bucket, grp, toUInt32(count()) AS active FROM (
            SELECT aor, argMax(grp, ts) AS grp, maxIf(toUnixTimestamp64Micro(ts) + expires_granted * 1000000, code BETWEEN 200 AND 299) AS expiry
            FROM registration_events WHERE ts > now64() - INTERVAL 1 DAY GROUP BY aor)
            WHERE expiry > toUnixTimestamp64Micro(now64(6)) GROUP BY grp");
        if let Err(e) = ch.exec(&sql, true).await {
            tracing::warn!("could not count active registrations: {e}");
        }
    }
}

async fn persist_sources(app: &App, ips: &[String]) {
    for ip in ips {
        let a = app.ingest.agents.read().unwrap().get(ip).cloned();
        let Some(a) = a else { continue };
        let _ = sqlx::query("INSERT INTO sources (ip, alias, capture_id, last_seen) VALUES (?, ?, ?, ?) ON DUPLICATE KEY UPDATE capture_id = VALUES(capture_id), last_seen = VALUES(last_seen)")
            .bind(&a.ip).bind(&a.alias).bind(a.capture_id).bind(crate::db::utc_now()).execute(&app.db).await;
    }
}

/// Aliases given in Admin survive restarts.
pub async fn load_sources(app: &App) -> Result<(), sqlx::Error> {
    let rows = sqlx::query("SELECT ip, alias, capture_id FROM sources").fetch_all(&app.db).await?;
    let mut agents = app.ingest.agents.write().unwrap();
    for r in rows {
        let ip: String = r.get("ip");
        agents.insert(ip.clone(), AgentStats { ip, alias: r.get("alias"), capture_id: r.get("capture_id"), ..AgentStats::default() });
    }
    Ok(())
}

pub async fn start(app: Arc<App>, rows: RowBox) -> Result<(), String> {
    tokio::spawn(writer(app.clone(), rows.0));
    tokio::spawn(minute_job(app.clone()));
    if !app.boot.hep_udp.is_empty() {
        udp_listener(app.clone(), app.boot.hep_udp.clone()).await?;
    }
    if !app.boot.hep_tcp.is_empty() {
        tcp_listener(app.clone(), app.boot.hep_tcp.clone()).await?;
    }
    Ok(())
}

pub fn private_ip(ip: &str) -> bool {
    is_private(ip)
}
