//! Single sign-on with OpenID Connect: the authorization code flow with PKCE, a nonce, and a
//! one-time state bound to the browser by a cookie. The ID token's signature, issuer, audience,
//! expiry and nonce are all checked before an account is signed in.

use crate::api::{App, problem};
use crate::auth::{self, User, client_ip, db_error};
use crate::db::{self, utc_now};
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type S = State<Arc<App>>;

const STATE_COOKIE: &str = "ss_sso_state";
const CALLBACK: &str = "/api/v1/auth/sso/callback";
/// A sign-in has this long to come back from the provider.
const LOGIN_TTL_MINUTES: i64 = 10;
/// Signing keys are cached this long, and fetched again at once when a token names an unknown key.
const KEYS_TTL: Duration = Duration::from_secs(3600);
const ALGORITHMS: &[Algorithm] = &[Algorithm::RS256, Algorithm::RS384, Algorithm::RS512, Algorithm::PS256, Algorithm::PS384, Algorithm::PS512, Algorithm::ES256, Algorithm::ES384];

#[derive(Clone)]
struct Provider {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    keys: JwkSet,
    fetched: Instant,
}

static PROVIDER: Mutex<Option<Provider>> = Mutex::new(None);

/// True when single sign-on is switched on and has what it needs to work.
pub fn enabled(a: &App) -> bool {
    a.settings.bool("sso_enabled") && !a.settings.get("sso_issuer").is_empty() && !a.settings.get("sso_client_id").is_empty()
}

/// What the sign-in page needs to know.
pub fn login_options(a: &App) -> Value {
    let on = enabled(a);
    json!({"enabled": on, "label": a.settings.get("sso_provider_name"), "password": !on || a.settings.bool("sso_allow_password")})
}

fn client_secret(a: &App) -> Result<String, String> {
    let stored = a.settings.get("sso_client_secret");
    if a.settings.locked("sso_client_secret") || stored.is_empty() {
        Ok(stored)
    } else {
        crate::storage::open(a, &stored).map_err(|e| e.replace("password", "client secret"))
    }
}

fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .map_err(|e| e.to_string())
}

/// The provider must be reached over HTTPS, except on this machine for testing.
fn check_url(url: &str) -> Result<reqwest::Url, String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| format!("{url} is not a valid URL."))?;
    let loopback = matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if parsed.scheme() == "https" || (parsed.scheme() == "http" && loopback) {
        Ok(parsed)
    } else {
        Err(format!("{url} must use https."))
    }
}

async fn get_json(client: &reqwest::Client, url: &str) -> Result<Value, String> {
    let res = client.get(check_url(url)?).header(header::ACCEPT, "application/json").send().await.map_err(|e| format!("Could not reach {url}: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("{url} answered {}.", res.status()));
    }
    res.json().await.map_err(|e| format!("{url} did not return JSON: {e}"))
}

/// Reads the provider's discovery document and signing keys, or returns the cached copy.
async fn provider(a: &App, refresh: bool) -> Result<Provider, String> {
    let issuer = a.settings.get("sso_issuer").trim().trim_end_matches('/').to_string();
    if let Some(p) = PROVIDER.lock().unwrap().clone()
        && !refresh
        && p.issuer == issuer
        && p.fetched.elapsed() < KEYS_TTL
    {
        return Ok(p);
    }
    let client = http()?;
    let doc = get_json(&client, &format!("{issuer}/.well-known/openid-configuration")).await?;
    let field = |k: &str| doc[k].as_str().map(str::to_string).ok_or_else(|| format!("The provider's configuration has no {k}."));
    // The document must describe the issuer it was fetched for, or tokens could come from elsewhere.
    if field("issuer")?.trim_end_matches('/') != issuer {
        return Err(format!("The provider says its issuer is {}, not {issuer}.", field("issuer")?));
    }
    let jwks_uri = field("jwks_uri")?;
    let keys: JwkSet = serde_json::from_value(get_json(&client, &jwks_uri).await?).map_err(|e| format!("The provider's signing keys could not be read: {e}"))?;
    let p = Provider {
        issuer: field("issuer")?,
        authorization_endpoint: check_url(&field("authorization_endpoint")?)?.to_string(),
        token_endpoint: check_url(&field("token_endpoint")?)?.to_string(),
        keys,
        fetched: Instant::now(),
    };
    *PROVIDER.lock().unwrap() = Some(p.clone());
    Ok(p)
}

fn random_hex() -> String {
    let mut raw = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut raw);
    hex::encode(raw)
}

fn redirect_uri(a: &App, headers: &HeaderMap) -> String {
    let fixed = a.settings.get("sso_redirect_url");
    if !fixed.is_empty() {
        return fixed;
    }
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("localhost");
    let https = headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()) == Some("https");
    format!("{}://{host}{CALLBACK}", if https { "https" } else { "http" })
}

fn state_cookie(a: &App, headers: &HeaderMap, value: &str, max_age: i64) -> String {
    let secure = match a.settings.get("cookie_secure").as_str() {
        "true" => true,
        "false" => false,
        _ => headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()) == Some("https"),
    };
    format!("{STATE_COOKIE}={value}; Path=/api/v1/auth/sso; HttpOnly; SameSite=Lax; Max-Age={max_age}{}", if secure { "; Secure" } else { "" })
}

fn see_other(location: &str, cookies: Vec<String>) -> Response {
    let mut res = (StatusCode::SEE_OTHER, [(header::LOCATION, location.to_string())]).into_response();
    for c in cookies {
        if let Ok(v) = c.parse() {
            res.headers_mut().append(header::SET_COOKIE, v);
        }
    }
    res
}

/// Back to the sign-in page with a message. The text is ours, never the provider's raw output.
fn fail(a: &App, headers: &HeaderMap, ip: &str, who: &str, message: &str, detail: &str) -> Response {
    db::audit(&a.db, who, ip, "auth.sso", "session", "failure", "warning", detail);
    tracing::warn!("single sign-on failed for {who}: {detail}");
    let mut url = reqwest::Url::parse("http://x/").expect("static URL");
    url.query_pairs_mut().append_pair("sso_error", message);
    see_other(&format!("/?{}", url.query().unwrap_or("")), vec![state_cookie(a, headers, "", 0)])
}

/// Starts a sign-in: remembers a one-time state, nonce and PKCE verifier, then sends the browser
/// to the provider.
pub async fn start(State(a): S, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Response {
    let ip = client_ip(&a, &headers, Some(peer));
    if !enabled(&a) {
        return fail(&a, &headers, &ip, "-", "Single sign-on is not set up.", "sign-in started while single sign-on is off");
    }
    let p = match provider(&a, false).await {
        Ok(p) => p,
        Err(e) => return fail(&a, &headers, &ip, "-", "The sign-in provider could not be reached. Try again, or ask an administrator.", &e),
    };
    let (state, nonce) = (random_hex(), random_hex());
    let mut raw = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut raw);
    let verifier = B64URL.encode(raw);
    let challenge = B64URL.encode(Sha256::digest(verifier.as_bytes()));
    let cutoff = utc_now() - chrono::Duration::minutes(LOGIN_TTL_MINUTES);
    let _ = sqlx::query("DELETE FROM sso_logins WHERE created_at < ?").bind(cutoff).execute(&a.db).await;
    if let Err(e) = sqlx::query("INSERT INTO sso_logins (state, nonce, verifier, created_at) VALUES (?, ?, ?, ?)").bind(&state).bind(&nonce).bind(&verifier).bind(utc_now()).execute(&a.db).await {
        return db_error(e);
    }
    let Ok(mut url) = reqwest::Url::parse(&p.authorization_endpoint) else {
        return fail(&a, &headers, &ip, "-", "The sign-in provider is not set up correctly.", "bad authorization endpoint");
    };
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &a.settings.get("sso_client_id"))
        .append_pair("redirect_uri", &redirect_uri(&a, &headers))
        .append_pair("scope", &a.settings.get("sso_scopes"))
        .append_pair("state", &state)
        .append_pair("nonce", &nonce)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");
    see_other(url.as_str(), vec![state_cookie(&a, &headers, &state, LOGIN_TTL_MINUTES * 60)])
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|part| {
        let (k, v) = part.trim().split_once('=')?;
        (k == name).then(|| v.to_string())
    })
}

fn same(x: &str, y: &str) -> bool {
    x.len() == y.len() && x.bytes().zip(y.bytes()).fold(0u8, |acc, (p, q)| acc | (p ^ q)) == 0
}

/// Checks the ID token's signature against the provider's keys, and its issuer, audience,
/// expiry and nonce.
async fn verify_id_token(a: &App, token: &str, nonce: &str) -> Result<Value, String> {
    let head = decode_header(token).map_err(|e| format!("ID token header: {e}"))?;
    if !ALGORITHMS.contains(&head.alg) {
        return Err(format!("ID token signed with {:?}, which is not accepted.", head.alg));
    }
    let client_id = a.settings.get("sso_client_id");
    let mut v = Validation::new(head.alg);
    v.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    v.set_audience(&[client_id.as_str()]);
    v.leeway = 60;
    // Cached keys first. When the key is unknown or the signature doesn't match, the provider may
    // have rotated its keys, so they are fetched again once before the token is refused.
    let mut claims = None;
    let mut last = String::new();
    for refresh in [false, true] {
        let p = provider(a, refresh).await?;
        let jwk = match &head.kid {
            Some(kid) => p.keys.find(kid).cloned(),
            None => p.keys.keys.first().cloned(),
        };
        let Some(jwk) = jwk else {
            last = "The ID token was signed with a key the provider doesn't publish.".into();
            continue;
        };
        let key = DecodingKey::from_jwk(&jwk).map_err(|e| format!("Signing key: {e}"))?;
        v.set_issuer(&[p.issuer.as_str()]);
        match decode::<Value>(token, &key, &v) {
            Ok(t) => {
                claims = Some(t.claims);
                break;
            }
            Err(e) if matches!(e.kind(), jsonwebtoken::errors::ErrorKind::InvalidSignature) => last = format!("ID token rejected: {e}"),
            Err(e) => return Err(format!("ID token rejected: {e}")),
        }
    }
    let claims = claims.ok_or(last)?;
    if !claims["nonce"].as_str().is_some_and(|n| same(n, nonce)) {
        return Err("ID token nonce does not match this sign-in.".into());
    }
    // With several audiences, the token must say it was issued to this application.
    if claims["aud"].as_array().is_some_and(|aud| aud.len() > 1) && claims["azp"].as_str() != Some(client_id.as_str()) {
        return Err("ID token was issued to another application.".into());
    }
    Ok(claims)
}

fn claim_text(claims: &Value, name: &str) -> Option<String> {
    claims[name].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// The role a sign-in maps to: the first Group=Role pair whose group the user has, or the
/// role for new accounts. `None` when no mapping is set, so an existing role is left alone.
fn mapped_role(a: &App, claims: &Value) -> Option<String> {
    let map = a.settings.get("sso_role_map");
    if map.trim().is_empty() {
        return None;
    }
    let groups: Vec<String> = match &claims[a.settings.get("sso_role_claim").as_str()] {
        Value::Array(v) => v.iter().filter_map(|g| g.as_str().map(str::to_string)).collect(),
        Value::String(s) => s.split([',', ' ']).filter(|g| !g.is_empty()).map(str::to_string).collect(),
        _ => vec![],
    };
    let hit = map.split(',').filter_map(|pair| pair.split_once('=')).map(|(g, r)| (g.trim(), r.trim())).find(|(g, _)| groups.iter().any(|x| x == g));
    Some(hit.map(|(_, r)| r.to_string()).unwrap_or_else(|| a.settings.get("sso_default_role")))
}

async fn role_id(a: &App, name: &str) -> Result<i64, String> {
    sqlx::query_scalar("SELECT id FROM roles WHERE name = ?").bind(name).fetch_optional(&a.db).await.map_err(|e| e.to_string())?.ok_or_else(|| format!("No role named {name} exists."))
}

/// Finds or creates the account for a verified sign-in. Accounts are matched by issuer and
/// subject only; a local account with the same name is never taken over.
async fn account(a: &App, claims: &Value) -> Result<(i64, String), (String, String)> {
    let sub = claim_text(claims, "sub").ok_or(("The provider did not identify the user.".to_string(), "no sub claim".to_string()))?;
    let iss = claim_text(claims, "iss").unwrap_or_default();
    let external = format!("{iss}|{sub}");
    if external.len() > 255 {
        return Err(("The provider's user ID is too long.".into(), "issuer and subject over 255 bytes".into()));
    }
    let username = claim_text(claims, &a.settings.get("sso_username_claim"))
        .or_else(|| claim_text(claims, "email"))
        .unwrap_or_else(|| sub.clone());
    let username: String = username.chars().filter(|c| !c.is_control() && !c.is_whitespace()).take(96).collect();
    let display = claim_text(claims, "name").unwrap_or_else(|| username.clone()).chars().take(128).collect::<String>();
    let email = claim_text(claims, "email").filter(|e| e.len() <= 190).unwrap_or_default();
    let mapped = mapped_role(a, claims);
    let internal = |e: sqlx::Error| ("Sign-in failed. Ask an administrator to check the server log.".to_string(), e.to_string());

    let linked = sqlx::query("SELECT id, username, status FROM users WHERE external_id = ?").bind(&external).fetch_optional(&a.db).await.map_err(internal)?;
    let found = match linked {
        Some(row) => Some(row),
        // An administrator may create the account in advance with the same user name and no
        // password; it is linked here. Accounts that have a password are never linked.
        None => sqlx::query("SELECT id, username, status FROM users WHERE username = ? AND auth_source = 'sso' AND external_id IS NULL").bind(&username).fetch_optional(&a.db).await.map_err(internal)?,
    };
    if let Some(row) = found {
        let id: i64 = row.get("id");
        if row.get::<String, _>("status") != "active" {
            return Err(("This account is disabled.".into(), format!("{username}: account disabled")));
        }
        let role = match &mapped {
            Some(name) => Some(role_id(a, name).await.map_err(|e| ("Your groups map to a role that doesn't exist. Ask an administrator.".to_string(), e))?),
            None => None,
        };
        sqlx::query("UPDATE users SET external_id = ?, display_name = ?, email = ?, role_id = COALESCE(?, role_id), updated_at = ? WHERE id = ?")
            .bind(&external).bind(&display).bind(&email).bind(role).bind(utc_now()).bind(id)
            .execute(&a.db).await.map_err(internal)?;
        return Ok((id, row.get("username")));
    }
    if !a.settings.bool("sso_auto_create") {
        return Err((format!("There is no account for {username}. Ask an administrator to create one."), format!("{username}: no account and automatic creation is off")));
    }
    let taken: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE username = ?").bind(&username).fetch_one(&a.db).await.map_err(internal)?;
    if taken > 0 {
        return Err((format!("An account named {username} already exists. Ask an administrator to link it."), format!("{username}: name taken by a password account")));
    }
    let role_name = mapped.unwrap_or_else(|| a.settings.get("sso_default_role"));
    let role = role_id(a, &role_name).await.map_err(|e| ("The role for new accounts doesn't exist. Ask an administrator.".to_string(), e))?;
    let now = utc_now();
    let res = sqlx::query("INSERT INTO users (username, display_name, email, password_hash, auth_source, external_id, role_id, grants, denies, prefs, created_at, updated_at) VALUES (?, ?, ?, '!', 'sso', ?, ?, '[]', '[]', '{}', ?, ?)")
        .bind(&username).bind(&display).bind(&email).bind(&external).bind(role).bind(now).bind(now)
        .execute(&a.db).await.map_err(internal)?;
    db::audit(&a.db, &username, "", "user.create", &username, "success", "notice", &format!("created by single sign-on with role {role_name}"));
    Ok((res.last_insert_id() as i64, username))
}

/// The provider sends the browser back here with a code. It is exchanged for an ID token, which
/// is verified before a session starts.
pub async fn callback(State(a): S, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, Query(q): Query<CallbackQuery>) -> Response {
    let ip = client_ip(&a, &headers, Some(peer));
    if let Some(e) = &q.error {
        let e: String = e.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(64).collect();
        return fail(&a, &headers, &ip, "-", "The sign-in provider refused the sign-in.", &format!("provider error {e}"));
    }
    let (Some(code), Some(state)) = (q.code, q.state) else {
        return fail(&a, &headers, &ip, "-", "The sign-in was incomplete. Try again.", "callback without code or state");
    };
    // The state must be the one this browser was given, and is used once.
    if !cookie(&headers, STATE_COOKIE).is_some_and(|c| same(&c, &state)) {
        return fail(&a, &headers, &ip, "-", "The sign-in expired or was started in another browser. Try again.", "state does not match the cookie");
    }
    let row = match sqlx::query("SELECT nonce, verifier, created_at FROM sso_logins WHERE state = ?").bind(&state).fetch_optional(&a.db).await {
        Ok(r) => r,
        Err(e) => return db_error(e),
    };
    let _ = sqlx::query("DELETE FROM sso_logins WHERE state = ?").bind(&state).execute(&a.db).await;
    let Some(row) = row.filter(|r| utc_now() - r.get::<chrono::NaiveDateTime, _>("created_at") < chrono::Duration::minutes(LOGIN_TTL_MINUTES)) else {
        return fail(&a, &headers, &ip, "-", "The sign-in expired. Try again.", "unknown or expired state");
    };
    let (nonce, verifier): (String, String) = (row.get("nonce"), row.get("verifier"));

    let p = match provider(&a, false).await {
        Ok(p) => p,
        Err(e) => return fail(&a, &headers, &ip, "-", "The sign-in provider could not be reached. Try again.", &e),
    };
    let secret = match client_secret(&a) {
        Ok(s) => s,
        Err(e) => return fail(&a, &headers, &ip, "-", "Single sign-on is not set up correctly. Ask an administrator.", &e),
    };
    let client_id = a.settings.get("sso_client_id");
    let redirect = redirect_uri(&a, &headers);
    let mut form = vec![("grant_type", "authorization_code"), ("code", code.as_str()), ("redirect_uri", redirect.as_str()), ("client_id", client_id.as_str()), ("code_verifier", verifier.as_str())];
    if !secret.is_empty() {
        form.push(("client_secret", secret.as_str()));
    }
    let exchanged = async {
        let res = http()?.post(&p.token_endpoint).header(header::ACCEPT, "application/json").form(&form).send().await.map_err(|e| format!("token request: {e}"))?;
        let status = res.status();
        let body: Value = res.json().await.map_err(|e| format!("token response: {e}"))?;
        if !status.is_success() {
            return Err(format!("token endpoint answered {status}: {}", body["error"].as_str().unwrap_or("")));
        }
        body["id_token"].as_str().map(str::to_string).ok_or_else(|| "token response has no id_token; is the openid scope set?".to_string())
    }
    .await;
    let token = match exchanged {
        Ok(t) => t,
        Err(e) => return fail(&a, &headers, &ip, "-", "The sign-in could not be completed. Ask an administrator to check the client ID and secret.", &e),
    };
    let claims = match verify_id_token(&a, &token, &nonce).await {
        Ok(c) => c,
        Err(e) => return fail(&a, &headers, &ip, "-", "The sign-in could not be verified.", &e),
    };
    let (id, username) = match account(&a, &claims).await {
        Ok(x) => x,
        Err((message, detail)) => return fail(&a, &headers, &ip, claim_text(&claims, "sub").as_deref().unwrap_or("-"), &message, &detail),
    };
    match auth::start_session(&a, &headers, id, &ip).await {
        Ok(session) => {
            db::audit(&a.db, &username, &ip, "auth.login", "session", "success", "info", "single sign-on");
            see_other("/", vec![session, state_cookie(&a, &headers, "", 0)])
        }
        Err(r) => r,
    }
}

/// Tries the provider's configuration from the Settings page.
pub async fn check(State(a): S, user: User, headers: HeaderMap) -> Response {
    if let Err(r) = user.need("settings_manage") {
        return r;
    }
    if a.settings.get("sso_issuer").is_empty() {
        return problem(StatusCode::BAD_REQUEST, "No issuer", "Enter the issuer URL first.");
    }
    match provider(&a, true).await {
        Ok(p) => Json(json!({
            "ok": true, "issuer": p.issuer, "authorization_endpoint": p.authorization_endpoint,
            "token_endpoint": p.token_endpoint, "keys": p.keys.keys.len(), "redirect_uri": redirect_uri(&a, &headers),
        }))
        .into_response(),
        Err(e) => problem(StatusCode::BAD_GATEWAY, "Provider not reachable", &e),
    }
}
