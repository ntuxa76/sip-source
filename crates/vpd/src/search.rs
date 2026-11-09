//! Universal search: type detection for pasted values and the `field:value` filter syntax, which
//! compiles to a parameter-free SQL condition with every value quoted.

use crate::sql::quote;
use serde::Serialize;

pub fn normalize(raw: &str) -> String {
    let d: String = raw.chars().filter(|c| c.is_ascii_digit() || *c == '+').collect();
    if let Some(r) = d.strip_prefix("+964") {
        return format!("964{r}");
    }
    if let Some(r) = d.strip_prefix("00964") {
        return format!("964{r}");
    }
    if d.len() == 11 && d.starts_with("07") {
        return format!("964{}", &d[1..]);
    }
    d.trim_start_matches('+').trim_start_matches("00").to_string()
}

pub const FIELDS: &[(&str, &str)] = &[
    ("caller", "Calling number"),
    ("callee", "Called number"),
    ("number", "Either party"),
    ("callid", "Any leg's Call-ID or correlation ID"),
    ("capture", "HEP capture ID or node name, e.g. capture:201"),
    ("backend", "asterisk, freeswitch, opensips, kamailio"),
    ("ip", "Caller or callee IP"),
    ("trunk", "Carrier name"),
    ("direction", "onnet, outbound, inbound, unknown"),
    ("status", "Caller-side final status, e.g. 503 or 4xx"),
    ("egress", "Status returned on the egress leg"),
    ("cause", "Q.850 cause"),
    ("end", "End cause, e.g. busy, cancelled"),
    ("hangup", "caller, callee or network"),
    ("state", "setup, ringing, answered, completed, failed, cancelled, timeout"),
    ("node", "Capture point"),
    ("group", "Ingress or egress topology group"),
    ("codec", "Negotiated codec"),
    ("ua", "User-agent text"),
    ("vendor", "User-agent vendor"),
    ("finding", "Diagnostic code"),
    ("pdd", "Post-dial delay, e.g. pdd>3s"),
    ("dur", "Talk time, e.g. dur<30s"),
    ("mos", "Worst MOS, e.g. mos<3.5"),
    ("loss", "Max loss %, e.g. loss>5"),
    ("last", "Time range, e.g. last:6h"),
];

#[derive(Clone, Serialize)]
pub struct Token {
    pub raw: String,
    pub field: String,
    pub op: String,
    pub value: String,
    pub detected: Option<String>,
    pub error: Option<String>,
    pub negated: bool,
}

fn canonical(field: &str) -> &str {
    match field {
        "captureid" | "hep" | "hepid" | "agent" => "capture",
        "cid" | "correlation" | "correlationid" | "xcid" => "callid",
        other => other,
    }
}

fn split(q: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for ch in q.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                cur.push(ch);
            }
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn split_op(t: &str) -> Option<(usize, &'static str)> {
    let first = t.find([':', '<', '>', '='])?;
    let rest = &t[first..];
    for op in [">=", "<=", ":", ">", "<", "="] {
        if rest.starts_with(op) {
            return Some((first, op));
        }
    }
    None
}

pub fn parse(q: &str) -> Vec<Token> {
    split(q).into_iter().map(|raw| {
        let is_uri = raw.starts_with("sip:") || raw.starts_with("tel:") || raw.starts_with("sips:");
        let (negated, body) = match raw.strip_prefix('-').or(raw.strip_prefix('!')) {
            Some(rest) if rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) => (true, rest.to_string()),
            _ => (false, raw.clone()),
        };
        if !is_uri
            && let Some((pos, op)) = split_op(&body)
            && pos > 0
            && body[..pos].chars().all(|c| c.is_ascii_alphabetic() || c == '-' || c == '_')
        {
            let lowered = body[..pos].to_lowercase().replace(['-', '_'], "");
            let field = canonical(&lowered).to_string();
            let value = body[pos + op.len()..].trim_matches('"').to_string();
            let known = FIELDS.iter().any(|(f, _)| *f == field);
            let error = (!known).then(|| format!("Unknown field \"{field}\". Try one of: caller, callee, capture, trunk, status, pdd, mos, finding."));
            return Token { raw: raw.clone(), field, op: op.to_string(), value, detected: None, error, negated };
        }
        detect(raw)
    }).collect()
}

fn is_ip_like(s: &str) -> bool {
    let core = s.split('/').next().unwrap_or("");
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| !p.is_empty() && p.parse::<u8>().is_ok())
}

fn looks_like_call_id(s: &str) -> bool {
    s.contains('@') || (s.len() >= 16 && s.chars().any(|c| c.is_ascii_alphabetic()) && s.chars().any(|c| c.is_ascii_digit()))
}

fn detect(raw: String) -> Token {
    let t = |field: &str, value: String, detected: String| Token { raw: raw.clone(), field: field.into(), op: ":".into(), value, detected: Some(detected), error: None, negated: false };
    if is_ip_like(&raw) {
        return t("ip", raw.clone(), "IP address".into());
    }
    if let Some(u) = raw.strip_prefix("sip:").or(raw.strip_prefix("sips:")).or(raw.strip_prefix("tel:")) {
        let user = u.split(['@', ';']).next().unwrap_or("");
        let n = normalize(user);
        return t("number", n.clone(), format!("SIP URI, number {n}"));
    }
    if looks_like_call_id(&raw) {
        return t("callid", raw.clone(), "Call-ID".into());
    }
    if let Some(s) = raw.strip_prefix('*')
        && s.chars().all(|c| c.is_ascii_digit())
        && !s.is_empty()
    {
        return t("number", raw.clone(), format!("number ending {s}"));
    }
    if let Some(s) = raw.strip_suffix('*')
        && s.chars().all(|c| c.is_ascii_digit())
        && !s.is_empty()
    {
        return t("number", format!("{}*", normalize(s)), format!("number prefix {s}"));
    }
    let digits = raw.chars().filter(|c| c.is_ascii_digit()).count();
    if digits >= 3 && raw.chars().all(|c| c.is_ascii_digit() || "+-() ".contains(c)) {
        let n = normalize(&raw);
        return t("number", n.clone(), format!("number, normalized to {n}"));
    }
    if raw.len() >= 3 {
        return t("ua", raw.clone(), "free text in user agent".into());
    }
    Token { raw: raw.clone(), field: String::new(), op: String::new(), value: raw, detected: None, error: Some("Too short to search for.".into()), negated: false }
}

fn duration_ms(v: &str) -> Option<f64> {
    let v = v.trim();
    let (num, mult) = if let Some(n) = v.strip_suffix("ms") {
        (n, 1.0)
    } else if let Some(n) = v.strip_suffix('s') {
        (n, 1000.0)
    } else if let Some(n) = v.strip_suffix('m') {
        (n, 60_000.0)
    } else if let Some(n) = v.strip_suffix('h') {
        (n, 3_600_000.0)
    } else if let Some(n) = v.strip_suffix('d') {
        (n, 86_400_000.0)
    } else {
        (v, 1.0)
    };
    num.parse::<f64>().ok().map(|x| x * mult)
}

/// The window from a `last:6h` token, in microseconds.
pub fn last_us(tokens: &[Token]) -> Option<i64> {
    tokens.iter().find(|t| t.field == "last" && t.error.is_none()).and_then(|t| duration_ms(&t.value)).map(|ms| (ms * 1000.0) as i64)
}

fn sql_cmp(op: &str) -> &str {
    if op == ":" { "=" } else { op }
}

fn sql_for(field: &str, op: &str, v: &str) -> Option<String> {
    let number = |col: &str| {
        Some(if let Some(s) = v.strip_prefix('*') {
            format!("{col} LIKE {}", quote(&format!("%{s}")))
        } else if let Some(p) = v.strip_suffix('*') {
            format!("{col} LIKE {}", quote(&format!("{}%", normalize(p))))
        } else {
            format!("{col} = {}", quote(&normalize(v)))
        })
    };
    let text = |col: &str| Some(format!("{col} = {}", quote(v)));
    match field {
        "caller" => number("caller_n"),
        "callee" => number("callee_n"),
        "number" => Some(format!("({} OR {})", number("caller_n")?, number("callee_n")?)),
        "callid" => Some(format!("arrayExists(l -> position(l, {}) > 0, legs)", quote(v))),
        "capture" => Some(match v.parse::<u32>() {
            Ok(id) => format!("has(capture_ids, {id})"),
            Err(_) => format!("has(capture_points, {})", quote(v)),
        }),
        "backend" => text("backend"),
        "ip" => {
            if v.contains('/') {
                Some(format!("(isIPAddressInRange(caller_ip, {0}) OR isIPAddressInRange(callee_ip, {0}))", quote(v)))
            } else {
                Some(format!("(caller_ip = {0} OR callee_ip = {0})", quote(v)))
            }
        }
        "trunk" => text("trunk"),
        "direction" => text("direction"),
        "status" | "egress" => {
            let col = if field == "egress" { "egress_status" } else { "final_status" };
            match v.to_lowercase().strip_suffix("xx").and_then(|d| d.parse::<u16>().ok()) {
                Some(k) => Some(format!("intDiv({col}, 100) = {k}")),
                None => v.parse::<u32>().ok().map(|n| format!("{col} {} {n}", sql_cmp(op))),
            }
        }
        "cause" => Some(format!("q850_cause = {}", v.parse::<u8>().unwrap_or(0))),
        "end" => text("end_cause"),
        "hangup" => text("hangup_side"),
        "state" => text("state"),
        "node" => Some(format!("(node = {0} OR pbx = {0} OR media_node = {0} OR has(capture_points, {0}))", quote(v))),
        "group" => Some(format!("(ingress_group = {0} OR egress_group = {0})", quote(v))),
        "codec" => Some(format!("has(codecs, {})", quote(v))),
        "ua" => Some(format!("(positionCaseInsensitive(ua_caller, {0}) > 0 OR positionCaseInsensitive(ua_callee, {0}) > 0)", quote(v))),
        "vendor" => text("ua_vendor"),
        "finding" => Some(format!("has(findings, {})", quote(v))),
        "pdd" => duration_ms(v).map(|n| format!("pdd_ms {} {n}", sql_cmp(op))),
        "dur" => duration_ms(v).map(|n| format!("talk_ms {} {n}", sql_cmp(op))),
        "mos" => v.parse::<f64>().ok().map(|n| format!("min_mos {} {n}", sql_cmp(op))),
        "loss" => v.parse::<f64>().ok().map(|n| format!("max_loss_pct {} {n}", sql_cmp(op))),
        _ => None,
    }
}

/// Several values of one field are alternatives (`status:503 status:486`, or `status:503,486`).
/// Different fields, ranges and negations must all hold. Empty when there is nothing to filter on.
pub fn to_sql(tokens: &[Token]) -> String {
    let mut by_field: Vec<(String, Vec<String>)> = Vec::new();
    let mut parts: Vec<String> = Vec::new();
    for tok in tokens.iter().filter(|t| t.error.is_none() && t.field != "last") {
        let values: Vec<&str> = if tok.op == ":" && tok.field != "callid" { tok.value.split(',').map(str::trim).filter(|v| !v.is_empty()).collect() } else { vec![tok.value.as_str()] };
        let conds: Vec<String> = values.iter().filter_map(|v| sql_for(&tok.field, &tok.op, v)).collect();
        if conds.is_empty() {
            continue;
        }
        if tok.negated {
            parts.push(format!("NOT ({})", conds.join(" OR ")));
        } else if tok.op == ":" {
            match by_field.iter_mut().find(|(f, _)| *f == tok.field) {
                Some((_, list)) => list.extend(conds),
                None => by_field.push((tok.field.clone(), conds)),
            }
        } else {
            parts.push(format!("({})", conds.join(" OR ")));
        }
    }
    parts.extend(by_field.into_iter().map(|(_, c)| format!("({})", c.join(" OR "))));
    parts.join(" AND ")
}

pub const FACETS: &[&str] = &["status", "capture", "backend", "trunk", "direction", "state", "finding", "vendor", "node", "codec", "group", "end", "cause"];

/// The SQL expression that produces a facet's values, one row per value.
pub fn facet_expr(name: &str) -> Option<&'static str> {
    Some(match name {
        "status" => "toString(final_status)",
        "capture" => "toString(arrayJoin(capture_ids))",
        "backend" => "backend",
        "trunk" => "trunk",
        "direction" => "direction",
        "state" => "state",
        "finding" => "arrayJoin(findings)",
        "vendor" => "ua_vendor",
        "node" => "node",
        "codec" => "arrayJoin(codecs)",
        "group" => "ingress_group",
        "end" => "end_cause",
        "cause" => "toString(q850_cause)",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_field_is_or_different_fields_are_and() {
        let s = to_sql(&parse("status:503 status:486 trunk:Carrier-X"));
        assert_eq!(s, "(final_status = 503 OR final_status = 486) AND (trunk = 'Carrier-X')");
    }

    #[test]
    fn quoting_cannot_break_out_of_a_value() {
        let s = to_sql(&parse("trunk:x'OR'1"));
        assert_eq!(s, "(trunk = 'x\\'OR\\'1')");
    }

    #[test]
    fn detects_pasted_values() {
        assert_eq!(parse("07701234567")[0].field, "number");
        assert_eq!(parse("07701234567")[0].value, "9647701234567");
        assert_eq!(parse("10.0.0.1")[0].field, "ip");
        assert_eq!(parse("a84b4c76e66710@pc33")[0].field, "callid");
        assert_eq!(parse("*2345")[0].value, "*2345");
    }

    #[test]
    fn negation_and_ranges() {
        assert_eq!(to_sql(&parse("-capture:201 pdd>3s")), "NOT (has(capture_ids, 201)) AND (pdd_ms > 3000)");
    }
}
