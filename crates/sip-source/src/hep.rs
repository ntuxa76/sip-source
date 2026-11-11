//! HEP (the SIP capture encapsulation protocol) v1, v2 and v3 decoding.

use flate2::read::{GzDecoder, ZlibDecoder};
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

const MAX_DECOMPRESSED: u64 = 256 * 1024;

#[derive(Debug, Default, Clone)]
pub struct Hep {
    pub version: u8,
    pub ip_proto: u8,
    pub src_ip: String,
    pub dst_ip: String,
    pub src_port: u16,
    pub dst_port: u16,
    pub ts_sec: u32,
    pub ts_usec: u32,
    pub proto_type: u8,
    pub capture_id: u32,
    pub auth_key: Option<String>,
    pub payload: Vec<u8>,
    pub correlation: Option<String>,
    pub agent_name: Option<String>,
}

fn be16(b: &[u8]) -> u16 {
    u16::from_be_bytes([b[0], b[1]])
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn inflate(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    let mut out = Vec::new();
    let read = if data.starts_with(&[0x1f, 0x8b]) {
        GzDecoder::new(data).take(MAX_DECOMPRESSED + 1).read_to_end(&mut out)
    } else {
        ZlibDecoder::new(data).take(MAX_DECOMPRESSED + 1).read_to_end(&mut out)
    };
    read.map_err(|_| "bad_compression")?;
    if out.len() as u64 > MAX_DECOMPRESSED {
        return Err("too_large");
    }
    Ok(out)
}

fn text(data: &[u8]) -> String {
    String::from_utf8_lossy(data).trim_end_matches('\0').to_string()
}

fn decode_v3(buf: &[u8]) -> Result<Hep, &'static str> {
    if buf.len() < 6 {
        return Err("bad_length");
    }
    let total = be16(&buf[4..6]) as usize;
    if total > buf.len() || total < 6 {
        return Err("bad_length");
    }
    let mut h = Hep { version: 3, ..Hep::default() };
    let mut seen_payload = false;
    let mut i = 6;
    while i + 6 <= total {
        let vendor = be16(&buf[i..]);
        let kind = be16(&buf[i + 2..]);
        let len = be16(&buf[i + 4..]) as usize;
        if len < 6 || i + len > total {
            return Err("bad_length");
        }
        let d = &buf[i + 6..i + len];
        if vendor == 0 {
            match kind {
                0x0002 if !d.is_empty() => h.ip_proto = d[0],
                0x0003 if d.len() == 4 => h.src_ip = Ipv4Addr::new(d[0], d[1], d[2], d[3]).to_string(),
                0x0004 if d.len() == 4 => h.dst_ip = Ipv4Addr::new(d[0], d[1], d[2], d[3]).to_string(),
                0x0005 if d.len() == 16 => h.src_ip = ipv6(d),
                0x0006 if d.len() == 16 => h.dst_ip = ipv6(d),
                0x0007 if d.len() == 2 => h.src_port = be16(d),
                0x0008 if d.len() == 2 => h.dst_port = be16(d),
                0x0009 if d.len() == 4 => h.ts_sec = be32(d),
                0x000a if d.len() == 4 => h.ts_usec = be32(d),
                0x000b if !d.is_empty() => h.proto_type = d[0],
                0x000c if d.len() == 4 => h.capture_id = be32(d),
                // Some agents send a 2-byte id.
                0x000c if d.len() == 2 => h.capture_id = be16(d) as u32,
                0x000e => h.auth_key = Some(text(d)),
                0x000f => {
                    h.payload = d.to_vec();
                    seen_payload = true;
                }
                0x0010 => {
                    h.payload = inflate(d)?;
                    seen_payload = true;
                }
                0x0011 => h.correlation = Some(text(d)).filter(|s| !s.is_empty()),
                0x0013 => h.agent_name = Some(text(d)).filter(|s| !s.is_empty()),
                _ => {}
            }
        }
        i += len;
    }
    if !seen_payload {
        return Err("missing_chunk");
    }
    Ok(h)
}

fn ipv6(d: &[u8]) -> String {
    let mut o = [0u8; 16];
    o.copy_from_slice(d);
    IpAddr::V6(Ipv6Addr::from(o)).to_string()
}

fn decode_legacy(buf: &[u8]) -> Result<Hep, &'static str> {
    let version = buf[0];
    let header_len = buf[1] as usize;
    if buf.len() < header_len || header_len < 8 {
        return Err("bad_length");
    }
    let family = buf[2];
    let mut h = Hep { version, ip_proto: buf[3], proto_type: 1, ..Hep::default() };
    h.src_port = be16(&buf[4..]);
    h.dst_port = be16(&buf[6..]);
    let mut i = 8;
    if family == 2 {
        if buf.len() < i + 8 {
            return Err("bad_length");
        }
        h.src_ip = Ipv4Addr::new(buf[i], buf[i + 1], buf[i + 2], buf[i + 3]).to_string();
        h.dst_ip = Ipv4Addr::new(buf[i + 4], buf[i + 5], buf[i + 6], buf[i + 7]).to_string();
        i += 8;
    } else {
        if buf.len() < i + 32 {
            return Err("bad_length");
        }
        h.src_ip = ipv6(&buf[i..i + 16]);
        h.dst_ip = ipv6(&buf[i + 16..i + 32]);
        i += 32;
    }
    if version == 2 {
        if buf.len() < i + 12 {
            return Err("bad_length");
        }
        h.ts_sec = u32::from_le_bytes([buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]);
        h.ts_usec = u32::from_le_bytes([buf[i + 4], buf[i + 5], buf[i + 6], buf[i + 7]]);
        h.capture_id = u16::from_le_bytes([buf[i + 8], buf[i + 9]]) as u32;
        i += 12;
    }
    h.payload = buf.get(i..).unwrap_or(&[]).to_vec();
    Ok(h)
}

pub fn decode(buf: &[u8]) -> Result<Hep, &'static str> {
    if buf.len() >= 4 && &buf[..4] == b"HEP3" {
        return decode_v3(buf);
    }
    match buf.first() {
        Some(1) | Some(2) if buf.len() > 8 => decode_legacy(buf),
        _ => Err("bad_magic"),
    }
}

/// How many bytes of a TCP stream the next HEP3 frame occupies, once enough has arrived.
pub fn frame_len(buf: &[u8]) -> Result<Option<usize>, &'static str> {
    if buf.len() < 6 {
        return Ok(None);
    }
    if &buf[..4] != b"HEP3" {
        return Err("bad_magic");
    }
    let total = be16(&buf[4..6]) as usize;
    if total < 6 {
        return Err("bad_length");
    }
    Ok((buf.len() >= total).then_some(total))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(kind: u16, data: &[u8]) -> Vec<u8> {
        let mut c = vec![0, 0];
        c.extend_from_slice(&kind.to_be_bytes());
        c.extend_from_slice(&((data.len() + 6) as u16).to_be_bytes());
        c.extend_from_slice(data);
        c
    }

    fn packet(chunks: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = chunks.concat();
        let mut p = b"HEP3".to_vec();
        p.extend_from_slice(&((body.len() + 6) as u16).to_be_bytes());
        p.extend(body);
        p
    }

    #[test]
    fn decodes_v3() {
        let p = packet(&[chunk(0x0002, &[17]), chunk(0x0003, &[10, 0, 0, 1]), chunk(0x0004, &[10, 0, 0, 2]), chunk(0x0007, &5060u16.to_be_bytes()), chunk(0x0008, &5061u16.to_be_bytes()), chunk(0x0009, &100u32.to_be_bytes()), chunk(0x000a, &7u32.to_be_bytes()), chunk(0x000b, &[1]), chunk(0x000c, &201u32.to_be_bytes()), chunk(0x000f, b"hello"), chunk(0x0011, b"abc")]);
        let h = decode(&p).unwrap();
        assert_eq!((h.src_ip.as_str(), h.dst_port, h.capture_id, h.proto_type), ("10.0.0.1", 5061, 201, 1));
        assert_eq!(h.payload, b"hello");
        assert_eq!(h.correlation.as_deref(), Some("abc"));
    }

    #[test]
    fn rejects_truncated_and_junk() {
        assert_eq!(decode(b"HEP3\xff\xff").unwrap_err(), "bad_length");
        assert_eq!(decode(b"junkdata").unwrap_err(), "bad_magic");
        let p = packet(&[chunk(0x000b, &[1])]);
        assert_eq!(decode(&p).unwrap_err(), "missing_chunk");
    }

    #[test]
    fn skips_unknown_chunks() {
        let p = packet(&[chunk(0x7777, &[1, 2, 3]), chunk(0x000f, b"x")]);
        assert_eq!(decode(&p).unwrap().payload, b"x");
    }

    #[test]
    fn frames_tcp_stream() {
        let p = packet(&[chunk(0x000f, b"abc")]);
        assert_eq!(frame_len(&p[..3]).unwrap(), None);
        assert_eq!(frame_len(&p).unwrap(), Some(p.len()));
    }

    /// Anyone who can reach the HEP port can send anything, so malformed input must be rejected,
    /// never panic. Truncations and random bytes, around a valid packet and on their own.
    #[test]
    fn hostile_input_never_panics() {
        use rand::{Rng, SeedableRng};
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let sip = b"INVITE sip:1@x SIP/2.0\r\nVia: SIP/2.0/UDP a;branch=z9hG4bK1\r\nCall-ID: c\r\nCSeq: 1 INVITE\r\nFrom: <sip:2@x>;tag=a\r\nTo: <sip:1@x>\r\nContent-Length: 4\r\n\r\nv=0\n";
        let valid = packet(&[chunk(0x0002, &[17]), chunk(0x0003, &[10, 0, 0, 1]), chunk(0x0004, &[10, 0, 0, 2]), chunk(0x000b, &[1]), chunk(0x000f, sip)]);
        for n in 0..=valid.len() {
            let _ = decode(&valid[..n]);
            let _ = frame_len(&valid[..n]);
        }
        for _ in 0..50_000 {
            let mut buf = if rng.gen_bool(0.5) { valid.clone() } else { (0..rng.gen_range(0..200)).map(|_| rng.r#gen()).collect() };
            for _ in 0..rng.gen_range(0..8) {
                if !buf.is_empty() {
                    let i = rng.gen_range(0..buf.len());
                    buf[i] = rng.r#gen();
                }
            }
            if rng.gen_bool(0.3) && buf.len() > 4 {
                buf[0..4].copy_from_slice(b"HEP3");
            }
            let _ = frame_len(&buf);
            if let Ok(h) = decode(&buf) {
                let _ = crate::sip::parse(&String::from_utf8_lossy(&h.payload));
            }
            let text = String::from_utf8_lossy(&buf);
            let _ = crate::sip::parse(&text);
            let _ = crate::sip::parse_sdp(&text);
        }
    }
}
