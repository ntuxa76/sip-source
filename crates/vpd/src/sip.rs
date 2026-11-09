//! Tolerant SIP and SDP parsing. A message that can't be parsed is kept, flagged as a parse error.

use serde::Serialize;

#[derive(Clone, Debug, Default, Serialize)]
pub struct SdpInfo {
    pub ip: String,
    pub port: u16,
    pub codecs: Vec<String>,
    pub direction: String,
    pub srtp: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Sip {
    pub is_request: bool,
    pub start_line: String,
    pub method: String,
    pub status: u16,
    pub reason: String,
    pub ruri_user: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub call_id: String,
    pub cseq_num: u32,
    pub cseq_method: String,
    pub from_user: String,
    pub from_domain: String,
    pub from_tag: String,
    pub to_user: String,
    pub to_domain: String,
    pub to_tag: String,
    pub pai_user: String,
    pub contact: String,
    pub contact_expires: Option<u32>,
    pub user_agent: String,
    pub via_branch: String,
    pub via_transport: String,
    pub expires: Option<u32>,
    pub q850: u8,
    pub sdp: Option<SdpInfo>,
}

fn canonical(name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "v" | "via" => "Via".into(),
        "f" | "from" => "From".into(),
        "t" | "to" => "To".into(),
        "i" | "call-id" => "Call-ID".into(),
        "m" | "contact" => "Contact".into(),
        "c" | "content-type" => "Content-Type".into(),
        "l" | "content-length" => "Content-Length".into(),
        "e" | "content-encoding" => "Content-Encoding".into(),
        "s" | "subject" => "Subject".into(),
        "k" | "supported" => "Supported".into(),
        "cseq" => "CSeq".into(),
        _ => name.to_string(),
    }
}

/// The user and domain of a SIP or tel URI, inside a name-addr or bare.
pub fn uri_parts(value: &str) -> (String, String) {
    let inner = match (value.find('<'), value.find('>')) {
        (Some(a), Some(b)) if a < b => &value[a + 1..b],
        _ => value.split(';').next().unwrap_or(value).trim(),
    };
    let rest = inner.strip_prefix("sips:").or_else(|| inner.strip_prefix("sip:")).or_else(|| inner.strip_prefix("tel:")).unwrap_or(inner);
    let rest = rest.split(';').next().unwrap_or(rest);
    match rest.split_once('@') {
        Some((user, domain)) => (user.split(':').next().unwrap_or(user).to_string(), domain.split(['?', '>']).next().unwrap_or(domain).to_string()),
        None if inner.starts_with("tel:") => (rest.to_string(), String::new()),
        None => (String::new(), rest.to_string()),
    }
}

/// A `;name=value` parameter, looking only outside the `<...>` part.
fn param(value: &str, name: &str) -> String {
    let tail = value.rfind('>').map(|i| &value[i + 1..]).unwrap_or(value);
    tail.split(';').skip(1).find_map(|p| {
        let (k, v) = p.split_once('=')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_string())
    }).unwrap_or_default()
}

fn codec_name(pt: &str) -> Option<&'static str> {
    Some(match pt {
        "0" => "PCMU",
        "3" => "GSM",
        "8" => "PCMA",
        "9" => "G722",
        "18" => "G729",
        _ => return None,
    })
}

pub fn parse_sdp(body: &str) -> Option<SdpInfo> {
    if !body.lines().any(|l| l.starts_with("m=")) {
        return None;
    }
    let mut s = SdpInfo { direction: "sendrecv".into(), ..SdpInfo::default() };
    let mut pts: Vec<String> = Vec::new();
    let mut rtpmap: Vec<(String, String)> = Vec::new();
    let mut in_audio = false;
    for line in body.lines().map(str::trim_end) {
        if let Some(c) = line.strip_prefix("c=") {
            if s.ip.is_empty() || in_audio {
                s.ip = c.split_whitespace().nth(2).unwrap_or("").to_string();
            }
        } else if let Some(m) = line.strip_prefix("m=") {
            let f: Vec<&str> = m.split_whitespace().collect();
            in_audio = f.first() == Some(&"audio");
            if in_audio && f.len() >= 4 {
                s.port = f[1].parse().unwrap_or(0);
                s.srtp |= f[2].contains("SAVP");
                pts = f[3..].iter().map(|x| x.to_string()).collect();
            }
        } else if in_audio && let Some(a) = line.strip_prefix("a=") {
            if let Some(map) = a.strip_prefix("rtpmap:") {
                if let Some((pt, rest)) = map.split_once(' ') {
                    rtpmap.push((pt.to_string(), rest.split('/').next().unwrap_or("").to_string()));
                }
            } else if matches!(a, "sendrecv" | "sendonly" | "recvonly" | "inactive") {
                s.direction = a.to_string();
            } else if a.starts_with("crypto:") || a.starts_with("fingerprint:") {
                s.srtp = true;
            }
        }
    }
    s.codecs = pts.iter().filter_map(|pt| {
        rtpmap.iter().find(|(p, _)| p == pt).map(|(_, n)| n.clone()).or_else(|| codec_name(pt).map(str::to_string))
    }).collect();
    Some(s)
}

/// Removes SDES key material before a message is stored (FR-DE-04).
pub fn mask_crypto(payload: &str) -> String {
    if !payload.contains("a=crypto:") {
        return payload.to_string();
    }
    payload.split_inclusive('\n').map(|line| {
        match (line.starts_with("a=crypto:"), line.find("inline:")) {
            (true, Some(i)) => format!("{}inline:****masked****\r\n", &line[..i]),
            _ => line.to_string(),
        }
    }).collect()
}

pub fn parse(text: &str) -> Option<Sip> {
    let (head, body) = match text.find("\r\n\r\n") {
        Some(i) => (&text[..i], &text[i + 4..]),
        None => match text.find("\n\n") {
            Some(i) => (&text[..i], &text[i + 2..]),
            None => (text, ""),
        },
    };
    let mut lines = head.lines();
    let start = lines.next()?.trim();
    let mut s = Sip { start_line: start.to_string(), body: body.to_string(), ..Sip::default() };
    if let Some(rest) = start.strip_prefix("SIP/2.0 ") {
        let (code, reason) = rest.split_once(' ').unwrap_or((rest, ""));
        s.status = code.parse().ok()?;
        s.reason = reason.to_string();
    } else {
        let mut parts = start.split_whitespace();
        s.method = parts.next()?.to_string();
        let uri = parts.next()?;
        if parts.next() != Some("SIP/2.0") {
            return None;
        }
        s.is_request = true;
        s.ruri_user = uri_parts(uri).0;
    }

    for line in lines {
        if line.starts_with([' ', '\t']) {
            if let Some(last) = s.headers.last_mut() {
                last.1.push(' ');
                last.1.push_str(line.trim());
            }
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            s.headers.push((canonical(k.trim()), v.trim().to_string()));
        }
    }

    for (name, value) in &s.headers {
        match name.as_str() {
            "Call-ID" => s.call_id = value.clone(),
            "CSeq" => {
                let mut p = value.split_whitespace();
                s.cseq_num = p.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                s.cseq_method = p.next().unwrap_or("").to_string();
            }
            "From" => {
                (s.from_user, s.from_domain) = uri_parts(value);
                s.from_tag = param(value, "tag");
            }
            "To" => {
                (s.to_user, s.to_domain) = uri_parts(value);
                s.to_tag = param(value, "tag");
            }
            "P-Asserted-Identity" if s.pai_user.is_empty() => s.pai_user = uri_parts(value).0,
            "Contact" if s.contact.is_empty() => {
                s.contact = value.clone();
                s.contact_expires = param(value, "expires").parse().ok();
            }
            "User-Agent" | "Server" if s.user_agent.is_empty() => s.user_agent = value.clone(),
            "Expires" => s.expires = value.parse().ok(),
            "Via" if s.via_branch.is_empty() => {
                s.via_branch = value.split(';').find_map(|p| p.trim().strip_prefix("branch=").map(str::to_string)).unwrap_or_default();
                s.via_transport = value.split_whitespace().next().and_then(|p| p.rsplit('/').next()).unwrap_or("").to_lowercase();
            }
            "Reason" if s.q850 == 0 && value.to_ascii_uppercase().starts_with("Q.850") => {
                s.q850 = value.split(';').find_map(|p| p.trim().strip_prefix("cause=").and_then(|c| c.parse().ok())).unwrap_or(0);
            }
            _ => {}
        }
    }
    if s.call_id.is_empty() {
        return None;
    }
    let is_sdp = s.headers.iter().any(|(k, v)| k == "Content-Type" && v.to_ascii_lowercase().contains("application/sdp"));
    if is_sdp {
        s.sdp = parse_sdp(&s.body);
    }
    Some(s)
}

pub fn header<'a>(s: &'a Sip, name: &str) -> Option<&'a str> {
    s.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    const INVITE: &str = "INVITE sip:07701234567@voice.example;user=phone SIP/2.0\r\nVia: SIP/2.0/UDP 10.0.0.1:5060;branch=z9hG4bKabc;rport\r\nf: \"Ali\" <sip:9647700012345@voice.example>;tag=aa1\r\nt: <sip:07701234567@voice.example>\r\ni: abc-123@10.0.0.1\r\nCSeq: 2 INVITE\r\nContact: <sip:u@10.0.0.1:5060>\r\nUser-Agent: Asterisk PBX 20.11.0\r\nX-CID: root-1\r\nContent-Type: application/sdp\r\nContent-Length: 90\r\n\r\nv=0\r\nc=IN IP4 10.0.0.9\r\nm=audio 12000 RTP/AVP 8 0 101\r\na=rtpmap:101 telephone-event/8000\r\na=sendonly\r\n";

    #[test]
    fn parses_invite_with_compact_headers() {
        let s = parse(INVITE).unwrap();
        assert!(s.is_request);
        assert_eq!((s.method.as_str(), s.ruri_user.as_str(), s.from_user.as_str(), s.from_tag.as_str()), ("INVITE", "07701234567", "9647700012345", "aa1"));
        assert_eq!((s.call_id.as_str(), s.cseq_num, s.cseq_method.as_str()), ("abc-123@10.0.0.1", 2, "INVITE"));
        assert_eq!(s.via_branch, "z9hG4bKabc");
        assert_eq!(header(&s, "x-cid"), Some("root-1"));
        let sdp = s.sdp.unwrap();
        assert_eq!((sdp.ip.as_str(), sdp.port, sdp.direction.as_str()), ("10.0.0.9", 12000, "sendonly"));
        assert_eq!(sdp.codecs, ["PCMA", "PCMU", "telephone-event"]);
    }

    #[test]
    fn parses_response_with_reason() {
        let s = parse("SIP/2.0 503 Service Unavailable\r\nCall-ID: x\r\nCSeq: 1 INVITE\r\nFrom: <sip:a@b>;tag=1\r\nTo: <sip:c@d>;tag=2\r\nReason: Q.850;cause=34;text=\"x\"\r\n\r\n").unwrap();
        assert_eq!((s.status, s.reason.as_str(), s.q850, s.to_tag.as_str()), (503, "Service Unavailable", 34, "2"));
    }

    #[test]
    fn rejects_junk_and_handles_tel() {
        assert!(parse("not sip at all").is_none());
        assert_eq!(uri_parts("tel:+9647700012345;phone-context=x").0, "+9647700012345");
        assert_eq!(uri_parts("sip:user:pw@host:5060;transport=tcp"), ("user".to_string(), "host:5060".to_string()));
    }

    #[test]
    fn masks_sdes_keys() {
        let m = mask_crypto("v=0\r\na=crypto:1 AES_CM_128_HMAC_SHA1_80 inline:SECRETKEY\r\na=sendrecv\r\n");
        assert!(!m.contains("SECRETKEY"));
        assert!(m.contains("a=sendrecv"));
    }
}
