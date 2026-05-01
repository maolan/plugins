use std::fs;
use std::io::{ErrorKind, Read};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use rand::{Rng, distributions::Alphanumeric, rngs::OsRng};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const DEFAULT_API_BASE: &str = "https://www.tone3000.com";
const DEFAULT_OAUTH_BASE: &str = "https://www.tone3000.com";
const DEFAULT_NAM_SEARCH_TEMPLATE: &str =
    "{base}/api/v1/tones/search?query={query}&page={page}&page_size={page_size}";
const DEFAULT_IR_SEARCH_TEMPLATE: &str =
    "{base}/api/v1/tones/search?query={query}&gears=ir&page={page}&page_size={page_size}";
const DEFAULT_NAM_DOWNLOAD_TEMPLATE: &str = "{base}/api/v1/models?tone_id={id}&page=1&page_size=25";
const DEFAULT_IR_DOWNLOAD_TEMPLATE: &str = "{base}/api/v1/models?tone_id={id}&page=1&page_size=25";
const CONFIG_SUBDIR: &str = "rural-modeler";
const OAUTH_FILE: &str = "tone3000_oauth.json";
const LEGACY_API_KEY_FILE: &str = "tone3000_access_token";
const TOKEN_EXPIRY_SKEW_SECS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    Nam,
    Ir,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchItem {
    pub id: String,
    pub name: String,
    pub variations: Vec<SearchVariation>,
    pub picture_url: Option<String>,
    pub picture: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchVariation {
    pub title: String,
    pub reference: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PaginatedSearchResults {
    pub items: Vec<SearchItem>,
    pub page: u32,
    pub total_pages: u32,
    pub total: u32,
}

#[derive(Debug, Clone)]
struct Config {
    base: String,
    auth_token: Option<String>,
    nam_search_template: String,
    ir_search_template: String,
    nam_download_template: String,
    ir_download_template: String,
}

impl Config {
    fn from_env() -> Self {
        let saved_token = resolve_saved_auth_token().unwrap_or_default();
        Self {
            base: std::env::var("TONE3000_API_BASE_URL")
                .unwrap_or_else(|_| DEFAULT_API_BASE.to_string()),
            auth_token: std::env::var("TONE3000_ACCESS_TOKEN")
                .ok()
                .or_else(|| std::env::var("TONE3000_API_KEY").ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .or(saved_token),
            nam_search_template: std::env::var("TONE3000_NAM_SEARCH_ENDPOINT_TEMPLATE")
                .unwrap_or_else(|_| DEFAULT_NAM_SEARCH_TEMPLATE.to_string()),
            ir_search_template: std::env::var("TONE3000_IR_SEARCH_ENDPOINT_TEMPLATE")
                .unwrap_or_else(|_| DEFAULT_IR_SEARCH_TEMPLATE.to_string()),
            nam_download_template: std::env::var("TONE3000_NAM_DOWNLOAD_ENDPOINT_TEMPLATE")
                .unwrap_or_else(|_| DEFAULT_NAM_DOWNLOAD_TEMPLATE.to_string()),
            ir_download_template: std::env::var("TONE3000_IR_DOWNLOAD_ENDPOINT_TEMPLATE")
                .unwrap_or_else(|_| DEFAULT_IR_DOWNLOAD_TEMPLATE.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthCredentials {
    pub client_id: String,
    pub redirect_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct OAuthSession {
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    redirect_uri: String,
    #[serde(default)]
    access_token: String,
    refresh_token: Option<String>,
    expires_at_unix: Option<u64>,
    pkce_verifier: Option<String>,
    pkce_state: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

pub fn has_valid_oauth_token() -> bool {
    resolve_saved_auth_token().ok().flatten().is_some()
}

pub fn load_saved_oauth_credentials() -> Result<Option<OAuthCredentials>, String> {
    let session = load_oauth_session()?;
    if session.client_id.trim().is_empty() && session.redirect_uri.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(OAuthCredentials {
        client_id: session.client_id,
        redirect_uri: session.redirect_uri,
    }))
}

pub fn begin_oauth_pkce_and_save(client_id: &str, redirect_uri: &str) -> Result<String, String> {
    let client_id = client_id.trim();
    if client_id.is_empty() {
        return Err("Tone3000 OAuth client_id is empty".to_string());
    }
    let redirect_uri = redirect_uri.trim();
    if redirect_uri.is_empty() {
        return Err("Tone3000 OAuth redirect_uri is empty".to_string());
    }

    let code_verifier = generate_pkce_verifier();
    let code_challenge = pkce_challenge_s256(&code_verifier);
    let state = random_state_token();
    let oauth_base =
        std::env::var("TONE3000_OAUTH_BASE_URL").unwrap_or_else(|_| DEFAULT_OAUTH_BASE.to_string());
    let authorize_url = format!(
        "{}/api/v1/oauth/authorize?client_id={}&redirect_uri={}&response_type=code&code_challenge={}&code_challenge_method=S256&state={}",
        oauth_base.trim_end_matches('/'),
        urlencoding::encode(client_id),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(&code_challenge),
        urlencoding::encode(&state),
    );

    let mut session = load_oauth_session()?;
    session.client_id = client_id.to_string();
    session.redirect_uri = redirect_uri.to_string();
    session.pkce_verifier = Some(code_verifier);
    session.pkce_state = Some(state);
    save_oauth_session(&session)?;
    Ok(authorize_url)
}

pub fn complete_oauth_callback_and_save(callback: &str) -> Result<(), String> {
    log_oauth("complete_oauth_callback_and_save start");
    let mut session = load_oauth_session().map_err(|e| {
        log_oauth(&format!("load_oauth_session failed: {e}"));
        e
    })?;
    let client_id = session.client_id.trim();
    if client_id.is_empty() {
        log_oauth("client_id empty");
        return Err("Tone3000 OAuth client_id is not set; run Begin OAuth first".to_string());
    }
    let session_redirect_uri = session.redirect_uri.trim();
    if session_redirect_uri.is_empty() {
        log_oauth("redirect_uri empty");
        return Err("Tone3000 OAuth redirect_uri is not set; run Begin OAuth first".to_string());
    }
    let callback_redirect_uri = extract_redirect_uri_from_callback(callback);
    let redirect_uri = callback_redirect_uri
        .as_deref()
        .unwrap_or(session_redirect_uri);
    let code_verifier = session
        .pkce_verifier
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            log_oauth("pkce_verifier missing");
            "Tone3000 PKCE verifier is missing; run Begin OAuth again".to_string()
        })?;
    let expected_state = session
        .pkce_state
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            log_oauth("pkce_state missing");
            "Tone3000 PKCE state is missing; run Begin OAuth again".to_string()
        })?;

    let query = extract_query_string(callback);
    log_oauth(&format!("callback query='{query}'"));
    let params = parse_query_pairs(query).map_err(|e| {
        log_oauth(&format!("parse_query_pairs failed: {e}"));
        e
    })?;
    let code = params
        .get("code")
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            log_oauth("code missing");
            if let Some(error) = params.get("error") {
                let desc = params
                    .get("error_description")
                    .map(String::as_str)
                    .unwrap_or_default();
                let mut msg = if desc.is_empty() {
                    format!("Tone3000 OAuth error: {error}")
                } else {
                    format!("Tone3000 OAuth error: {error}: {desc}")
                };
                if desc.to_ascii_lowercase().contains("redirect_uri") {
                    msg.push_str(&format!(
                        " — register {session_redirect_uri} as a redirect URI in your Tone3000 app settings at tone3000.com"
                    ));
                }
                return msg;
            }
            "Tone3000 OAuth callback is missing 'code'".to_string()
        })?;
    let state = params
        .get("state")
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            log_oauth("state missing");
            "Tone3000 OAuth callback is missing 'state'".to_string()
        })?;
    if state != expected_state {
        log_oauth(&format!(
            "state mismatch: got='{state}' expected='{expected_state}'"
        ));
        return Err("Tone3000 OAuth state mismatch; run Begin OAuth again".to_string());
    }

    log_oauth("requesting token");
    let token = request_token_authorization_code_with_loopback_fallback(
        client_id,
        code,
        code_verifier,
        redirect_uri,
    )
    .map_err(|e| {
        log_oauth(&format!("token request failed: {e}"));
        e
    })?;
    if let Some(callback_redirect_uri) = callback_redirect_uri {
        session.redirect_uri = callback_redirect_uri;
    }
    session.access_token = token.access_token;
    session.refresh_token = token.refresh_token;
    session.expires_at_unix = token
        .expires_in
        .map(|ttl| unix_now_secs().saturating_add(ttl));
    session.pkce_verifier = None;
    session.pkce_state = None;
    log_oauth("saving session");
    save_oauth_session(&session).map_err(|e| {
        log_oauth(&format!("save_oauth_session failed: {e}"));
        e
    })
}

fn log_oauth(msg: &str) {
    let line = format!(
        "{}  {msg}\n",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    );
    let path = std::env::var("HOME").ok().and_then(|h| {
        let p = std::path::PathBuf::from(h)
            .join(".config")
            .join("rural-modeler")
            .join("oauth-debug.log");
        std::fs::create_dir_all(p.parent()?).ok();
        Some(p)
    });
    if let Some(path) = path {
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
    }
}

pub fn oauth_login_with_browser(client_id: &str) -> Result<(), String> {
    let client_id = client_id.trim();
    if client_id.is_empty() {
        return Err("Tone3000 OAuth client_id is empty".to_string());
    }

    // Tone3000 requires a fixed redirect URI; ephemeral ports fail with
    // "Failed to register redirect_uri". Try a small set of well-known ports.
    const FIXED_PORTS: [u16; 5] = [8765, 8766, 8767, 8768, 8769];
    let mut listener_v4 = None;
    let mut port = 0;
    for p in FIXED_PORTS {
        match TcpListener::bind(format!("127.0.0.1:{p}")) {
            Ok(l) => {
                listener_v4 = Some(l);
                port = p;
                break;
            }
            Err(_) => continue,
        }
    }
    let listener_v4 = listener_v4.ok_or_else(|| {
        "Failed to start local OAuth callback listener on ports 8765-8769".to_string()
    })?;
    listener_v4
        .set_nonblocking(true)
        .map_err(|e| format!("Failed to configure local OAuth callback listener: {e}"))?;

    let listener_v6 = TcpListener::bind(format!("[::1]:{port}")).ok();
    if let Some(ref l) = listener_v6 {
        let _ = l.set_nonblocking(true);
    }

    let has_v6 = listener_v6.is_some();
    log_oauth(&format!("oauth start  port={port} ipv6={has_v6}"));

    let redirect_uri = format!("http://localhost:{port}/");
    let authorize_url = begin_oauth_pkce_and_save(client_id, &redirect_uri)?;
    log_oauth(&format!("authorize_url={authorize_url}"));
    open_url_in_browser(&authorize_url)?;
    log_oauth("browser opened");
    let callback_query =
        wait_for_oauth_callback_query(&listener_v4, listener_v6.as_ref(), Duration::from_secs(180));
    match &callback_query {
        Ok(q) => log_oauth(&format!("callback received: {q}")),
        Err(e) => log_oauth(&format!("callback error: {e}")),
    }
    // Keep answering follow-up browser requests (favicon, etc.) for a short
    // grace period so the browser doesn't see connection refused.
    drain_oauth_listeners(&listener_v4, listener_v6.as_ref());
    complete_oauth_callback_and_save(&callback_query?)
}

fn generate_pkce_verifier() -> String {
    let mut rng = OsRng;
    let mut verifier = String::with_capacity(64);
    while verifier.len() < 64 {
        let c: char = rng.sample(Alphanumeric) as char;
        verifier.push(c);
    }
    verifier
}

fn random_state_token() -> String {
    let mut rng = OsRng;
    let mut state = String::with_capacity(32);
    while state.len() < 32 {
        let c: char = rng.sample(Alphanumeric) as char;
        state.push(c);
    }
    state
}

fn pkce_challenge_s256(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

fn extract_query_string(callback: &str) -> &str {
    let trimmed = callback.trim();
    if let Some(pos) = trimmed.find('?') {
        let rest = &trimmed[pos + 1..];
        if let Some(hash_pos) = rest.find('#') {
            &rest[..hash_pos]
        } else {
            rest
        }
    } else {
        trimmed.trim_start_matches('?')
    }
}

fn extract_redirect_uri_from_callback(callback: &str) -> Option<String> {
    let trimmed = callback.trim();
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return None;
    }
    let without_fragment = trimmed.split('#').next().unwrap_or(trimmed);
    let without_query = without_fragment
        .split_once('?')
        .map(|(base, _)| base)
        .unwrap_or(without_fragment);
    let redirect_uri = without_query.trim();
    if redirect_uri.is_empty() {
        None
    } else {
        Some(redirect_uri.to_string())
    }
}

fn parse_query_pairs(query: &str) -> Result<std::collections::HashMap<String, String>, String> {
    let mut out = std::collections::HashMap::new();
    for pair in query.split('&') {
        if pair.trim().is_empty() {
            continue;
        }
        let mut parts = pair.splitn(2, '=');
        let key_raw = parts.next().unwrap_or_default();
        let value_raw = parts.next().unwrap_or_default();
        let key = urlencoding::decode(key_raw)
            .map_err(|e| format!("Failed to decode OAuth query key '{key_raw}': {e}"))?
            .into_owned();
        let value = urlencoding::decode(value_raw)
            .map_err(|e| format!("Failed to decode OAuth query value for key '{key}': {e}"))?
            .into_owned();
        out.insert(key, value);
    }
    Ok(out)
}

fn wait_for_oauth_callback_query(
    listener_v4: &TcpListener,
    listener_v6: Option<&TcpListener>,
    timeout: Duration,
) -> Result<String, String> {
    let start = std::time::Instant::now();
    let mut req_buf = [0_u8; 8192];
    while start.elapsed() < timeout {
        if let Some(result) = try_accept_oauth_callback(listener_v4, &mut req_buf) {
            return result;
        }
        if let Some(l) = listener_v6
            && let Some(result) = try_accept_oauth_callback(l, &mut req_buf)
        {
            return result;
        }
        thread::sleep(Duration::from_millis(1));
    }
    Err("Timed out waiting for Tone3000 OAuth callback".to_string())
}

fn try_accept_oauth_callback(
    listener: &TcpListener,
    req_buf: &mut [u8; 8192],
) -> Option<Result<String, String>> {
    match listener.accept() {
        Ok((mut stream, addr)) => {
            log_oauth(&format!("accept from {addr}"));
            let _ = stream.set_nonblocking(true);
            let n = stream.read(req_buf).unwrap_or(0);
            log_oauth(&format!("read {n} bytes from {addr}"));
            let request = std::str::from_utf8(&req_buf[..n]).unwrap_or_default();
            let target = request
                .lines()
                .next()
                .and_then(|line| {
                    let mut parts = line.split_whitespace();
                    let method = parts.next()?;
                    if method != "GET" {
                        return None;
                    }
                    parts.next().map(str::to_string)
                })
                .unwrap_or_default();
            // Only process the root path; ignore favicon and other browser noise.
            let is_root = target == "/" || target.starts_with("/?");
            let query = target.split('?').nth(1);
            let response_body = if !is_root {
                "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\n\r\n".to_string()
            } else {
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\n\r\nTone3000 authentication completed. You can close this tab.".to_string()
            };
            let _ = std::io::Write::write_all(&mut stream, response_body.as_bytes());
            let _ = std::io::Write::flush(&mut stream);

            if !is_root {
                log_oauth(&format!("ignored non-root request from {addr}: '{target}'"));
                return None;
            }
            if let Some(query) = query {
                log_oauth(&format!("found query from {addr}: ?{query}"));
                return Some(Ok(format!("?{query}")));
            }
            log_oauth(&format!("no query from {addr}, target='{target}'"));
            None
        }
        Err(err) if err.kind() == ErrorKind::WouldBlock => None,
        Err(err) => {
            log_oauth(&format!("accept error: {err}"));
            Some(Err(format!("OAuth callback listener failed: {err}")))
        }
    }
}

fn drain_oauth_listeners(listener_v4: &TcpListener, listener_v6: Option<&TcpListener>) {
    let deadline = std::time::Instant::now() + Duration::from_millis(500);
    let success = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\n\r\nTone3000 authentication completed. You can close this tab.";
    while std::time::Instant::now() < deadline {
        if let Ok((mut stream, addr)) = listener_v4.accept() {
            log_oauth(&format!("drain accept from {addr}"));
            let _ = stream.set_nonblocking(true);
            let _ = std::io::Write::write_all(&mut stream, success);
            let _ = std::io::Write::flush(&mut stream);
        }
        if let Some(l) = listener_v6
            && let Ok((mut stream, addr)) = l.accept()
        {
            log_oauth(&format!("drain accept from {addr}"));
            let _ = stream.set_nonblocking(true);
            let _ = std::io::Write::write_all(&mut stream, success);
            let _ = std::io::Write::flush(&mut stream);
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn open_url_in_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map_err(|e| format!("Failed to open browser for OAuth URL: {e}"))?;
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("Failed to open browser for OAuth URL: {e}"))?;
        return Ok(());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut attempts = Vec::new();

        let openers: [(&str, &[&str]); 4] = [
            ("xdg-open", &[url]),
            ("gio", &["open", url]),
            ("open", &[url]),
            ("firefox", &[url]),
        ];
        for (program, args) in openers {
            match run_browser_opener(program, args) {
                Ok(()) => return Ok(()),
                Err(err) => attempts.push(err),
            }
        }

        return Err(format!(
            "Failed to launch browser automatically. Tried xdg-open/gio/open/firefox. Details: {}",
            attempts.join(" | ")
        ));
    }
    #[allow(unreachable_code)]
    Err("Unsupported platform for automatic browser launch".to_string())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn run_browser_opener(program: &str, args: &[&str]) -> Result<(), String> {
    let _child = Command::new(program)
        .args(args)
        .spawn()
        .map_err(|e| format!("{program} start failed: {e}"))?;
    Ok(())
}

fn config_dir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME")
        .map(|v| v.trim().to_string())
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "HOME is not set; cannot resolve ~/.config/rural-modeler".to_string())?;
    Ok(PathBuf::from(home).join(".config").join(CONFIG_SUBDIR))
}

fn oauth_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join(OAUTH_FILE))
}

pub fn clear_oauth_credentials() -> Result<(), String> {
    let path = oauth_path()?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!(
            "Failed to delete Tone3000 OAuth config '{}': {err}",
            path.display()
        )),
    }
}

fn legacy_api_key_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join(LEGACY_API_KEY_FILE))
}

fn load_oauth_session() -> Result<OAuthSession, String> {
    let path = oauth_path()?;
    match fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str::<OAuthSession>(&raw).map_err(|e| {
            format!(
                "Failed to parse Tone3000 OAuth config '{}': {e}",
                path.display()
            )
        }),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(OAuthSession::default()),
        Err(err) => Err(format!(
            "Failed to read Tone3000 OAuth config '{}': {err}",
            path.display()
        )),
    }
}

fn save_oauth_session(session: &OAuthSession) -> Result<(), String> {
    let path = oauth_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "Failed to create Tone3000 config directory '{}': {err}",
                parent.display()
            )
        })?;
    }
    let data = serde_json::to_string_pretty(session).map_err(|e| {
        format!(
            "Failed to serialize Tone3000 OAuth config '{}': {e}",
            path.display()
        )
    })?;
    fs::write(&path, format!("{data}\n")).map_err(|err| {
        format!(
            "Failed to write Tone3000 OAuth config '{}': {err}",
            path.display()
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(|err| {
            format!(
                "Failed to set permissions on Tone3000 OAuth config '{}': {err}",
                path.display()
            )
        })?;
    }
    Ok(())
}

fn resolve_saved_auth_token() -> Result<Option<String>, String> {
    let mut session = load_oauth_session()?;
    if session.access_token.trim().is_empty() {
        return load_legacy_api_key();
    }

    let now = unix_now_secs();
    let expires_at = session.expires_at_unix.unwrap_or(u64::MAX);
    let is_fresh = expires_at.saturating_sub(TOKEN_EXPIRY_SKEW_SECS) > now;
    if is_fresh {
        return Ok(Some(session.access_token));
    }

    if let (Some(refresh_token), false) = (
        session
            .refresh_token
            .clone()
            .filter(|s| !s.trim().is_empty()),
        session.client_id.trim().is_empty(),
    ) {
        let token = request_token_refresh(session.client_id.trim(), refresh_token.trim())?;
        session.access_token = token.access_token;
        session.refresh_token = token.refresh_token.or(session.refresh_token);
        session.expires_at_unix = token
            .expires_in
            .map(|ttl| unix_now_secs().saturating_add(ttl));
        save_oauth_session(&session)?;
        return Ok(Some(session.access_token));
    }

    Ok(Some(session.access_token))
}

fn load_legacy_api_key() -> Result<Option<String>, String> {
    let path = legacy_api_key_path()?;
    match fs::read_to_string(&path) {
        Ok(raw) => {
            let token = raw.trim().to_string();
            if token.is_empty() {
                Ok(None)
            } else {
                Ok(Some(token))
            }
        }
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!(
            "Failed to read legacy Tone3000 token from '{}': {err}",
            path.display()
        )),
    }
}

fn oauth_token_url() -> String {
    let base =
        std::env::var("TONE3000_OAUTH_BASE_URL").unwrap_or_else(|_| DEFAULT_OAUTH_BASE.to_string());
    format!("{}/api/v1/oauth/token", base.trim_end_matches('/'))
}

fn request_token_authorization_code(
    client_id: &str,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> Result<OAuthTokenResponse, String> {
    post_oauth_form(&[
        ("grant_type", "authorization_code"),
        ("client_id", client_id),
        ("code", code),
        ("code_verifier", code_verifier),
        ("redirect_uri", redirect_uri),
    ])
}

fn request_token_authorization_code_with_loopback_fallback(
    client_id: &str,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> Result<OAuthTokenResponse, String> {
    match request_token_authorization_code(client_id, code, code_verifier, redirect_uri) {
        Ok(token) => Ok(token),
        Err(primary_err) => {
            let invalid_redirect = primary_err.contains("invalid_grant")
                && primary_err.contains("Invalid redirect_uri");
            if !invalid_redirect {
                return Err(primary_err);
            }
            let alternates = alternate_loopback_redirect_uris(redirect_uri);
            if alternates.is_empty() {
                return Err(primary_err);
            }
            let mut last_err = primary_err.clone();
            for alt_redirect_uri in &alternates {
                match request_token_authorization_code(
                    client_id,
                    code,
                    code_verifier,
                    alt_redirect_uri,
                ) {
                    Ok(token) => return Ok(token),
                    Err(alt_err) => {
                        last_err = format!(
                            "{last_err}; retry with alternate redirect_uri '{alt_redirect_uri}' also failed: {alt_err}"
                        );
                    }
                }
            }
            Err(last_err)
        }
    }
}

fn alternate_loopback_redirect_uris(uri: &str) -> Vec<String> {
    const LOOPBACK_HOSTS: [&str; 3] = ["localhost", "127.0.0.1", "[::1]"];
    let mut alternates = Vec::new();
    for host in LOOPBACK_HOSTS {
        let needle = format!("://{host}");
        if uri.contains(&needle) {
            for alt_host in LOOPBACK_HOSTS {
                if alt_host != host {
                    alternates.push(uri.replacen(&needle, &format!("://{alt_host}"), 1));
                }
            }
            break;
        }
    }
    alternates
}

fn request_token_refresh(
    client_id: &str,
    refresh_token: &str,
) -> Result<OAuthTokenResponse, String> {
    let params = vec![
        ("grant_type", "refresh_token"),
        ("client_id", client_id),
        ("refresh_token", refresh_token),
    ];
    post_oauth_form(&params)
}

fn post_oauth_form(params: &[(&str, &str)]) -> Result<OAuthTokenResponse, String> {
    let url = oauth_token_url();
    let response = match ureq::post(&url)
        .set("User-Agent", "rural-modeler/0.1")
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_form(params)
    {
        Ok(response) => response,
        Err(ureq::Error::Status(code, response)) => {
            let retry_after = response.header("Retry-After").map(String::from);
            let mut body = String::new();
            let mut reader = response.into_reader();
            let _ = reader.read_to_string(&mut body);
            let body = body.trim();
            let retry_after_str = retry_after.as_deref().unwrap_or("");
            if code == 429 {
                log_oauth(&format!(
                    "RATE LIMIT on {url}: retry_after='{retry_after_str}' body='{body}'"
                ));
                return Err(format!(
                    "Tone3000 rate limit hit (retry after {retry_after_str}s)"
                ));
            }
            if body.is_empty() {
                return Err(format!(
                    "Tone3000 OAuth request failed for '{url}': status code {code}"
                ));
            }
            return Err(format!(
                "Tone3000 OAuth request failed for '{url}': status code {code}; body: {body}"
            ));
        }
        Err(err) => {
            return Err(format!("Tone3000 OAuth request failed for '{url}': {err}"));
        }
    };
    let mut body = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut body)
        .map_err(|e| format!("Failed to read Tone3000 OAuth response body from '{url}': {e}"))?;
    serde_json::from_slice::<OAuthTokenResponse>(&body)
        .map_err(|e| format!("Tone3000 OAuth response is not valid JSON: {e}"))
}

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn search(
    kind: AssetKind,
    query: &str,
    page: u32,
    page_size: u32,
    gears: Option<&str>,
) -> Result<PaginatedSearchResults, String> {
    let config = Config::from_env();
    let encoded = urlencoding::encode(query).into_owned();
    let template = match kind {
        AssetKind::Nam => &config.nam_search_template,
        AssetKind::Ir => &config.ir_search_template,
    };
    let mut url = render_template(template, &config.base, &encoded, page, page_size);
    if let Some(gears) = gears
        && !gears.is_empty()
    {
        url.push_str("&gears=");
        url.push_str(&urlencoding::encode(gears));
    }
    let mut fallback_urls = Vec::new();
    if let Some(url) = fallback_search_url(kind, &url) {
        fallback_urls.push(url);
    }
    if let Some(url) = host_fallback_url(&url) {
        fallback_urls.push(url);
    }
    if let Some(url) = fallback_urls.first()
        && let Some(host_fallback) = host_fallback_url(url)
        && !fallback_urls.iter().any(|u| u == &host_fallback)
    {
        fallback_urls.push(host_fallback);
    }

    let body = match get_bytes(&url, config.auth_token.as_deref(), 2) {
        Ok(body) => body,
        Err(primary_err) => {
            let mut last_error = primary_err.clone();
            let mut fallback_body: Option<Vec<u8>> = None;
            let mut tried_any_fallback = false;
            for alt_url in fallback_urls {
                tried_any_fallback = true;
                match get_bytes(&alt_url, config.auth_token.as_deref(), 2) {
                    Ok(body) => {
                        fallback_body = Some(body);
                        break;
                    }
                    Err(alt_err) => {
                        last_error =
                            format!("{last_error}; fallback '{alt_url}' also failed: {alt_err}");
                    }
                }
            }
            if let Some(body) = fallback_body {
                body
            } else if !tried_any_fallback {
                return Err(add_search_hint(kind, primary_err));
            } else {
                return Err(add_search_hint(kind, last_error));
            }
        }
    };
    let value: Value = serde_json::from_slice(&body)
        .map_err(|e| format!("Tone3000 search response is not valid JSON: {e}"))?;
    let (page, total_pages, total) = extract_pagination(&value);
    let mut results = extract_search_results(&value);
    for item in &mut results {
        item.variations = fetch_tone_variations(kind, &config, &item.id);
        if let Some(url) = &item.picture_url
            && let Ok(bytes) = get_bytes(url, config.auth_token.as_deref(), 2)
        {
            item.picture = Some(bytes);
        }
    }
    Ok(PaginatedSearchResults {
        items: results,
        page,
        total_pages,
        total,
    })
}

fn host_fallback_url(url: &str) -> Option<String> {
    if url.contains("://api.tone3000.com/") {
        Some(url.replacen("://api.tone3000.com/", "://www.tone3000.com/", 1))
    } else if url.contains("://www.tone3000.com/") {
        Some(url.replacen("://www.tone3000.com/", "://api.tone3000.com/", 1))
    } else {
        None
    }
}

pub fn download_to_temp(kind: AssetKind, reference: &str) -> Result<PathBuf, String> {
    let config = Config::from_env();
    let (url, ext) = if reference.starts_with("http://") || reference.starts_with("https://") {
        (
            reference.to_string(),
            match kind {
                AssetKind::Nam => "nam",
                AssetKind::Ir => "wav",
            },
        )
    } else {
        let template = match kind {
            AssetKind::Nam => &config.nam_download_template,
            AssetKind::Ir => &config.ir_download_template,
        };
        (
            render_template(
                template,
                &config.base,
                &urlencoding::encode(reference),
                1,
                25,
            ),
            match kind {
                AssetKind::Nam => "nam",
                AssetKind::Ir => "wav",
            },
        )
    };

    let body = get_download_payload(kind, &url, config.auth_token.as_deref(), 3)?;
    let stem = sanitize_filename(reference);
    let file_name = format!(
        "{}-{}-{}.{}",
        match kind {
            AssetKind::Nam => "tone3000-model",
            AssetKind::Ir => "tone3000-ir",
        },
        stem,
        unix_ms_now(),
        ext
    );
    let dir = std::env::temp_dir().join("rural-modeler-tone3000");
    fs::create_dir_all(&dir).map_err(|e| {
        format!(
            "Failed to create temporary Tone3000 download directory '{}': {e}",
            dir.display()
        )
    })?;
    let path = dir.join(file_name);
    fs::write(&path, body)
        .map_err(|e| format!("Failed to write downloaded file '{}': {e}", path.display()))?;
    Ok(path)
}

fn render_template(
    template: &str,
    base: &str,
    encoded_value: &str,
    page: u32,
    page_size: u32,
) -> String {
    template
        .replace("{base}", base)
        .replace("{id}", encoded_value)
        .replace("{query}", encoded_value)
        .replace("{page}", &page.to_string())
        .replace("{page_size}", &page_size.to_string())
}

fn get_download_payload(
    kind: AssetKind,
    url: &str,
    auth_token: Option<&str>,
    depth: usize,
) -> Result<Vec<u8>, String> {
    if depth == 0 {
        return Err("Tone3000 download redirection depth exceeded".to_string());
    }
    let body = get_bytes(url, auth_token, depth)?;
    let looks_like_json = body
        .iter()
        .copied()
        .find(|b| !b.is_ascii_whitespace())
        .is_some_and(|b| b == b'{' || b == b'[');
    if !looks_like_json {
        return Ok(body);
    }

    let json: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return Ok(body),
    };
    if let Some(next_url) = extract_download_url(kind, &json) {
        return get_download_payload(kind, &next_url, auth_token, depth - 1);
    }
    if kind == AssetKind::Nam && looks_like_nam_payload_json(&json) {
        return Ok(body);
    }
    Err("Tone3000 returned JSON without a downloadable binary URL".to_string())
}

fn looks_like_nam_payload_json(value: &Value) -> bool {
    value.is_object()
        && value.get("version").is_some()
        && value
            .get("metadata")
            .is_some_and(|meta| meta.is_object() || meta.is_null())
}

fn get_bytes(url: &str, auth_token: Option<&str>, depth: usize) -> Result<Vec<u8>, String> {
    if depth == 0 {
        return Err("Tone3000 request redirection depth exceeded".to_string());
    }

    let agent: ureq::Agent = ureq::builder()
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(60))
        .timeout_write(std::time::Duration::from_secs(60))
        .build();

    let mut request = agent.get(url).set("User-Agent", "rural-modeler/0.1");
    if let Some(key) = auth_token {
        request = request
            .set("Authorization", &format!("Bearer {key}"))
            .set("X-API-Key", key);
    }
    let response = match request.call() {
        Ok(response) => response,
        Err(ureq::Error::Status(code, response)) => {
            let retry_after = response.header("Retry-After").map(String::from);
            let mut body = Vec::new();
            let _ = response.into_reader().read_to_end(&mut body);
            let preview = preview_bytes(&body, 512);
            let retry_after_str = retry_after.as_deref().unwrap_or("");
            if code == 429 {
                log_oauth(&format!(
                    "RATE LIMIT on {url}: retry_after='{retry_after_str}' preview='{preview}'"
                ));
                return Err(format!(
                    "Tone3000 rate limit hit (retry after {retry_after_str}s)"
                ));
            }
            return Err(format!(
                "Tone3000 request failed for '{url}': status code {code}; body_preview: {preview}"
            ));
        }
        Err(err) => return Err(format!("Tone3000 request failed for '{url}': {err}")),
    };
    let mut body = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut body)
        .map_err(|e| format!("Failed to read Tone3000 response body from '{url}': {e}"))?;
    Ok(body)
}

fn preview_bytes(bytes: &[u8], max: usize) -> String {
    let clipped = &bytes[..bytes.len().min(max)];
    let mut text = String::from_utf8_lossy(clipped).into_owned();
    text = text.replace('\n', "\\n").replace('\r', "\\r");
    if bytes.len() > max {
        text.push_str("...(truncated)");
    }
    text
}

fn extract_pagination(value: &Value) -> (u32, u32, u32) {
    let page = value.get("page").and_then(Value::as_u64).unwrap_or(1) as u32;
    let total_pages = value
        .get("total_pages")
        .and_then(Value::as_u64)
        .unwrap_or(1) as u32;
    let total = value.get("total").and_then(Value::as_u64).unwrap_or(0) as u32;
    (page, total_pages, total)
}

fn fallback_search_url(kind: AssetKind, url: &str) -> Option<String> {
    match kind {
        AssetKind::Nam => {
            if url.contains("/names/") {
                Some(url.replacen("/names/", "/nams/", 1))
            } else {
                None
            }
        }
        AssetKind::Ir => None,
    }
}

fn add_search_hint(kind: AssetKind, error: String) -> String {
    match kind {
        AssetKind::Nam if error.contains("/names/") && error.contains("404") => format!(
            "{error}. Hint: check TONE3000_NAM_SEARCH_ENDPOINT_TEMPLATE. Did you mean '/api/v1/nams/search'?"
        ),
        _ => error,
    }
}

fn extract_download_url(kind: AssetKind, value: &Value) -> Option<String> {
    let keys = [
        "model_url",
        "modelUrl",
        "download_url",
        "downloadUrl",
        "file_url",
        "fileUrl",
        "asset_url",
        "assetUrl",
        "url",
    ];

    for key in keys {
        if let Some(url) = value.get(key).and_then(Value::as_str)
            && !url.is_empty()
        {
            return Some(url.to_string());
        }
    }

    if let Some(arr) = value.as_array()
        && let Some(url) = find_download_url_in_array(kind, arr)
    {
        return Some(url);
    }
    if let Some(arr) = value.get("items").and_then(Value::as_array)
        && let Some(url) = find_download_url_in_array(kind, arr)
    {
        return Some(url);
    }
    if let Some(arr) = value.get("results").and_then(Value::as_array)
        && let Some(url) = find_download_url_in_array(kind, arr)
    {
        return Some(url);
    }
    if let Some(arr) = value.get("data").and_then(Value::as_array)
        && let Some(url) = find_download_url_in_array(kind, arr)
    {
        return Some(url);
    }

    if let Some(obj) = value.get("data") {
        return extract_download_url(kind, obj);
    }
    if let Some(obj) = value.get("result") {
        return extract_download_url(kind, obj);
    }
    None
}

fn find_download_url_in_array(kind: AssetKind, arr: &[Value]) -> Option<String> {
    let wanted_platform = match kind {
        AssetKind::Nam => "nam",
        AssetKind::Ir => "ir",
    };

    for entry in arr {
        if entry
            .get("platform")
            .and_then(Value::as_str)
            .is_some_and(|p| p.eq_ignore_ascii_case(wanted_platform))
            && let Some(url) = extract_download_url(kind, entry)
        {
            return Some(url);
        }
    }
    for entry in arr {
        if let Some(url) = extract_download_url(kind, entry) {
            return Some(url);
        }
    }
    None
}

fn extract_picture_url(entry: &Value) -> Option<String> {
    // Tone3000 returns tone images as an array of URLs under "images"
    if let Some(images) = entry.get("images").and_then(Value::as_array)
        && let Some(url) = images.first().and_then(Value::as_str)
        && !url.is_empty()
    {
        return Some(url.to_string());
    }

    // User avatar can be nested under "user.avatar_url"
    if let Some(url) = entry
        .get("user")
        .and_then(|u| u.get("avatar_url"))
        .and_then(Value::as_str)
        && !url.is_empty()
    {
        return Some(url.to_string());
    }

    // Fallback to common single-string picture fields
    let keys = [
        "avatar_url",
        "picture",
        "image",
        "thumbnail",
        "photo",
        "profile_picture",
        "cover",
        "cover_url",
        "artwork",
        "artwork_url",
        "picture_url",
        "image_url",
    ];
    for key in keys {
        if let Some(url) = entry.get(key).and_then(Value::as_str)
            && !url.is_empty()
        {
            return Some(url.to_string());
        }
    }
    None
}

fn extract_search_results(value: &Value) -> Vec<SearchItem> {
    let arrays = [
        value.as_array(),
        value.get("items").and_then(Value::as_array),
        value.get("results").and_then(Value::as_array),
        value.get("data").and_then(Value::as_array),
        value
            .get("data")
            .and_then(|d| d.get("items"))
            .and_then(Value::as_array),
        value
            .get("data")
            .and_then(|d| d.get("results"))
            .and_then(Value::as_array),
    ];

    for arr_opt in arrays {
        let Some(arr) = arr_opt else {
            continue;
        };
        let mut out = Vec::new();
        for entry in arr {
            let id = entry
                .get("id")
                .or_else(|| entry.get("slug"))
                .or_else(|| entry.get("uuid"))
                .or_else(|| entry.get("name"))
                .and_then(value_to_string)
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            if id.is_empty() {
                continue;
            }

            let name = entry
                .get("name")
                .or_else(|| entry.get("title"))
                .or_else(|| entry.get("display_name"))
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .trim()
                .to_string();
            let picture_url = extract_picture_url(entry);
            out.push(SearchItem {
                id,
                name,
                variations: Vec::new(),
                picture: None,
                picture_url,
            });
            if out.len() >= 20 {
                break;
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    Vec::new()
}

fn fetch_tone_variations(kind: AssetKind, config: &Config, tone_id: &str) -> Vec<SearchVariation> {
    let template = match kind {
        AssetKind::Nam => &config.nam_download_template,
        AssetKind::Ir => &config.ir_download_template,
    };
    let url = render_template(template, &config.base, &urlencoding::encode(tone_id), 1, 25);
    let body = match get_bytes(&url, config.auth_token.as_deref(), 2) {
        Ok(body) => body,
        Err(_) => {
            return Vec::new();
        }
    };
    let value: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return Vec::new();
        }
    };
    let arrays = [
        value.get("data").and_then(Value::as_array),
        value.get("items").and_then(Value::as_array),
        value.get("results").and_then(Value::as_array),
        value.as_array(),
    ];

    let mut out = Vec::new();
    for arr_opt in arrays {
        let Some(arr) = arr_opt else {
            continue;
        };
        for entry in arr {
            let reference = entry
                .get("model_url")
                .or_else(|| entry.get("download_url"))
                .or_else(|| entry.get("downloadUrl"))
                .or_else(|| entry.get("file_url"))
                .or_else(|| entry.get("fileUrl"))
                .or_else(|| entry.get("url"))
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("")
                .to_string();
            if reference.is_empty() {
                continue;
            }
            if !variation_matches_kind(kind, entry, &reference) {
                continue;
            }
            let title = entry
                .get("name")
                .or_else(|| entry.get("title"))
                .or_else(|| entry.get("display_name"))
                .or_else(|| entry.get("id"))
                .and_then(value_to_string)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Untitled".to_string());
            if out
                .iter()
                .any(|v: &SearchVariation| v.reference == reference)
            {
                continue;
            }
            out.push(SearchVariation { title, reference });
        }
        if !out.is_empty() {
            break;
        }
    }
    out
}

fn variation_matches_kind(kind: AssetKind, entry: &Value, reference: &str) -> bool {
    if let Some(platform) = entry.get("platform").and_then(Value::as_str) {
        return match kind {
            AssetKind::Nam => platform.eq_ignore_ascii_case("nam"),
            AssetKind::Ir => platform.eq_ignore_ascii_case("ir"),
        };
    }

    let lower = reference.to_ascii_lowercase();
    match kind {
        AssetKind::Nam => lower.ends_with(".nam") || lower.contains(".nam?"),
        AssetKind::Ir => lower.ends_with(".wav") || lower.contains(".wav?"),
    }
}

fn value_to_string(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        Some(s.to_string())
    } else if let Some(n) = value.as_u64() {
        Some(n.to_string())
    } else {
        value.as_i64().map(|n| n.to_string())
    }
}

fn sanitize_filename(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "asset".to_string()
    } else {
        trimmed.to_string()
    }
}

fn unix_ms_now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn _is_probably_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_results_from_items_list() {
        let value = json!({
            "items": [
                {"id": "abc", "name": "Lead"},
                {"id": "def", "title": "IR 1"}
            ]
        });
        let out = extract_search_results(&value);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].id, "abc");
        assert_eq!(out[0].name, "Lead");
        assert_eq!(out[1].id, "def");
        assert_eq!(out[1].name, "IR 1");
    }

    #[test]
    fn extracts_results_from_nested_data_results() {
        let value = json!({
            "data": {
                "results": [
                    {"slug": "foo", "display_name": "Foo", "summary": "desc"}
                ]
            }
        });
        let out = extract_search_results(&value);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "foo");
        assert_eq!(out[0].name, "Foo");
        assert!(out[0].variations.is_empty());
    }

    #[test]
    fn fallback_search_url_corrects_names_typo_for_nam() {
        let url = "https://api.tone3000.com/api/v1/names/search?q=engl";
        let out = fallback_search_url(AssetKind::Nam, url);
        assert_eq!(
            out.as_deref(),
            Some("https://api.tone3000.com/api/v1/nams/search?q=engl")
        );
    }

    #[test]
    fn fallback_search_url_is_none_for_ir() {
        let url = "https://api.tone3000.com/api/v1/irs/search?q=room";
        assert_eq!(fallback_search_url(AssetKind::Ir, url), None);
    }

    #[test]
    fn extract_download_url_prefers_matching_platform_from_data_array() {
        let value = json!({
            "data": [
                {"id": 1, "platform": "ir", "model_url": "https://cdn.example.com/ir.wav"},
                {"id": 2, "platform": "nam", "model_url": "https://cdn.example.com/model.nam"}
            ]
        });
        let nam_url = extract_download_url(AssetKind::Nam, &value);
        let ir_url = extract_download_url(AssetKind::Ir, &value);
        assert_eq!(
            nam_url.as_deref(),
            Some("https://cdn.example.com/model.nam")
        );
        assert_eq!(ir_url.as_deref(), Some("https://cdn.example.com/ir.wav"));
    }

    #[test]
    fn extract_download_url_falls_back_to_first_model_url() {
        let value = json!({
            "items": [
                {"id": 1, "model_url": "https://cdn.example.com/first.nam"},
                {"id": 2, "model_url": "https://cdn.example.com/second.nam"}
            ]
        });
        let url = extract_download_url(AssetKind::Nam, &value);
        assert_eq!(url.as_deref(), Some("https://cdn.example.com/first.nam"));
    }

    #[test]
    fn extract_search_results_accepts_numeric_id() {
        let value = json!({
            "data": [
                {"id": 41446, "title": "ENGL E650", "description": "Amp capture"}
            ]
        });
        let out = extract_search_results(&value);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "41446");
        assert_eq!(out[0].name, "ENGL E650");
    }

    #[test]
    fn extract_picture_url_prefers_images_array() {
        let value = json!({
            "images": ["https://cdn.example.com/tone.jpg"],
            "avatar_url": "https://cdn.example.com/avatar.jpg"
        });
        assert_eq!(
            extract_picture_url(&value),
            Some("https://cdn.example.com/tone.jpg".to_string())
        );
    }

    #[test]
    fn extract_picture_url_falls_back_to_user_avatar() {
        let value = json!({
            "user": {"avatar_url": "https://cdn.example.com/user.jpg"}
        });
        assert_eq!(
            extract_picture_url(&value),
            Some("https://cdn.example.com/user.jpg".to_string())
        );
    }

    #[test]
    fn extract_picture_url_returns_none_when_missing() {
        let value = json!({"id": 1, "name": "No pics"});
        assert_eq!(extract_picture_url(&value), None);
    }

    #[test]
    fn extract_search_results_includes_picture_url() {
        let value = json!({
            "items": [
                {"id": "abc", "name": "Lead", "images": ["https://cdn.example.com/lead.jpg"]}
            ]
        });
        let out = extract_search_results(&value);
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].picture_url,
            Some("https://cdn.example.com/lead.jpg".to_string())
        );
    }

    #[test]
    fn extract_query_string_handles_full_url_and_fragment() {
        let query = extract_query_string("http://localhost/callback?code=abc&state=xyz#frag");
        assert_eq!(query, "code=abc&state=xyz");
    }

    #[test]
    fn extract_redirect_uri_from_callback_full_url() {
        let uri = extract_redirect_uri_from_callback(
            "http://localhost:64792/callback?code=abc&state=xyz#frag",
        );
        assert_eq!(uri.as_deref(), Some("http://localhost:64792/callback"));
    }

    #[test]
    fn extract_redirect_uri_from_callback_query_only_returns_none() {
        let uri = extract_redirect_uri_from_callback("?code=abc&state=xyz");
        assert_eq!(uri, None);
    }

    #[test]
    fn parse_query_pairs_decodes_url_encoding() {
        let params = parse_query_pairs("code=abc123&state=s%2B1").expect("query parse");
        assert_eq!(params.get("code").map(String::as_str), Some("abc123"));
        assert_eq!(params.get("state").map(String::as_str), Some("s+1"));
    }

    #[test]
    fn pkce_challenge_has_expected_shape() {
        let challenge = pkce_challenge_s256("verifier");
        assert!(!challenge.contains('='));
        assert!(challenge.len() >= 43);
    }
}
