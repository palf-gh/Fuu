//! OAuth 2.0 for Gmail and Microsoft (Outlook.com / Microsoft 365) mail.
//!
//! Authorisation code flow with PKCE (S256) and a loopback redirect: Fuu listens
//! on 127.0.0.1 (and ::1 for Microsoft's `localhost`) on a random port, opens the
//! system browser (the app does that, on an event from the engine), and takes the
//! first redirect that carries the expected `state`. Tokens: the refresh token is
//! kept in the OS keychain, the access token only in memory.
//!
//! Client IDs are the user's own registrations, read from `oauth.toml` in Fuu's
//! config directory or from environment variables; none ship with Fuu.

use std::path::Path;
use std::time::Duration;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    Google,
    Microsoft,
}

impl Provider {
    pub fn name(self) -> &'static str {
        match self {
            Provider::Google => "Google",
            Provider::Microsoft => "Microsoft",
        }
    }

    fn authorise_endpoint(self) -> &'static str {
        match self {
            Provider::Google => "https://accounts.google.com/o/oauth2/v2/auth",
            Provider::Microsoft => "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
        }
    }

    fn token_endpoint(self) -> &'static str {
        match self {
            Provider::Google => "https://oauth2.googleapis.com/token",
            Provider::Microsoft => "https://login.microsoftonline.com/common/oauth2/v2.0/token",
        }
    }

    fn scopes(self) -> &'static str {
        match self {
            Provider::Google => "https://mail.google.com/",
            Provider::Microsoft => {
                "https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send offline_access"
            }
        }
    }

    /// Host used in the redirect URI. Google accepts any port on 127.0.0.1 for
    /// desktop clients; Entra ignores the port for `http://localhost`.
    fn redirect_host(self) -> &'static str {
        match self {
            Provider::Google => "127.0.0.1",
            Provider::Microsoft => "localhost",
        }
    }

    /// Environment variable prefix (`FUU_GOOGLE_CLIENT_ID`, …).
    fn env_prefix(self) -> &'static str {
        match self {
            Provider::Google => "FUU_GOOGLE",
            Provider::Microsoft => "FUU_MICROSOFT",
        }
    }
}

/// One registered OAuth client.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct ClientConfig {
    pub client_id: String,
    /// Google "Desktop app" clients have a secret that is not confidential but is
    /// still required on the token call. Microsoft public clients have none.
    #[serde(default)]
    pub client_secret: Option<String>,
}

/// Contents of `oauth.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct OAuthClients {
    #[serde(default)]
    pub google: Option<ClientConfig>,
    #[serde(default)]
    pub microsoft: Option<ClientConfig>,
}

/// File name of the client configuration, next to `accounts.toml`.
pub const CLIENTS_FILE: &str = "oauth.toml";

impl OAuthClients {
    /// Read `oauth.toml` (if present), then let environment variables override.
    pub fn load(path: &Path) -> Self {
        let mut clients: OAuthClients =
            std::fs::read_to_string(path).ok().and_then(|text| toml::from_str(&text).ok()).unwrap_or_default();
        for provider in [Provider::Google, Provider::Microsoft] {
            let prefix = provider.env_prefix();
            if let Ok(id) = std::env::var(format!("{prefix}_CLIENT_ID")) {
                let secret = std::env::var(format!("{prefix}_CLIENT_SECRET")).ok();
                *clients.slot(provider) = Some(ClientConfig { client_id: id, client_secret: secret });
            }
        }
        clients
    }

    fn slot(&mut self, provider: Provider) -> &mut Option<ClientConfig> {
        match provider {
            Provider::Google => &mut self.google,
            Provider::Microsoft => &mut self.microsoft,
        }
    }

    pub fn get(&self, provider: Provider) -> Option<&ClientConfig> {
        match provider {
            Provider::Google => self.google.as_ref(),
            Provider::Microsoft => self.microsoft.as_ref(),
        }
        .filter(|c| !c.client_id.trim().is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OAuthError {
    /// The refresh token was revoked or expired: the user must sign in again.
    InvalidGrant(String),
    Cancelled,
    TimedOut,
    Denied(String),
    Other(String),
}

impl std::fmt::Display for OAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidGrant(e) => write!(f, "再ログインが必要です（{e}）"),
            Self::Cancelled => f.write_str("ログインを取り消しました"),
            Self::TimedOut => f.write_str("ブラウザでのログインが時間内に完了しませんでした"),
            Self::Denied(e) => write!(f, "ログインが許可されませんでした（{e}）"),
            Self::Other(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for OAuthError {}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Random URL-safe token of `bytes` random bytes.
pub fn random_token(bytes: usize) -> String {
    use ring::rand::SecureRandom;
    let mut buf = vec![0u8; bytes];
    ring::rand::SystemRandom::new().fill(&mut buf).expect("system random");
    b64url(&buf)
}

/// PKCE S256 challenge for a verifier (RFC 7636).
pub fn pkce_challenge(verifier: &str) -> String {
    b64url(ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes()).as_ref())
}

/// Percent-encode for application/x-www-form-urlencoded and query strings.
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                match std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()).ok_or(()) {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", url_encode(k), url_encode(v))).collect::<Vec<_>>().join("&")
}

/// The browser URL that starts the sign-in.
pub fn authorise_url(provider: Provider, client: &ClientConfig, redirect_uri: &str, state: &str, challenge: &str, login_hint: &str) -> String {
    let mut pairs = vec![
        ("response_type", "code"),
        ("client_id", client.client_id.as_str()),
        ("redirect_uri", redirect_uri),
        ("scope", provider.scopes()),
        ("state", state),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
    ];
    if !login_hint.is_empty() {
        pairs.push(("login_hint", login_hint));
    }
    if provider == Provider::Google {
        // Ask for a refresh token every time (Google issues it only on consent).
        pairs.push(("access_type", "offline"));
        pairs.push(("prompt", "consent"));
    }
    format!("{}?{}", provider.authorise_endpoint(), form(&pairs))
}

/// Outcome of one request to the loopback listener.
#[derive(Debug, PartialEq, Eq)]
pub enum RedirectOutcome {
    /// Not the redirect (favicon, wrong path, wrong state): keep waiting.
    Ignore,
    Code(String),
    Error(String),
}

/// Interpret the request line of a loopback request, e.g. `GET /?code=…&state=… HTTP/1.1`.
pub fn parse_redirect(request_line: &str, expected_state: &str) -> RedirectOutcome {
    let mut parts = request_line.split_whitespace();
    if parts.next() != Some("GET") {
        return RedirectOutcome::Ignore;
    }
    let Some(target) = parts.next() else { return RedirectOutcome::Ignore };
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != "/" {
        return RedirectOutcome::Ignore;
    }
    let mut code = None;
    let mut state = None;
    let mut error = None;
    let mut description = None;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        match k {
            "code" => code = Some(url_decode(v)),
            "state" => state = Some(url_decode(v)),
            "error" => error = Some(url_decode(v)),
            "error_description" => description = Some(url_decode(v)),
            _ => {}
        }
    }
    if state.as_deref() != Some(expected_state) {
        return RedirectOutcome::Ignore;
    }
    if let Some(e) = error {
        return RedirectOutcome::Error(description.map_or(e.clone(), |d| format!("{e}: {d}")));
    }
    code.filter(|c| !c.is_empty()).map_or(RedirectOutcome::Ignore, RedirectOutcome::Code)
}

const DONE_PAGE: &str = "<!DOCTYPE html><html lang=\"ja\"><head><meta charset=\"utf-8\"><title>Fuu</title></head>\
<body style=\"font-family:system-ui,sans-serif;padding:48px;color:#1b1b1a\"><h2>Fuu</h2>\
<p>ログインの処理を Fuu に渡しました。このタブは閉じて構いません。</p></body></html>";

/// A loopback listener waiting for the browser redirect.
pub struct Loopback {
    listeners: Vec<TcpListener>,
    pub redirect_uri: String,
}

impl Loopback {
    /// Bind a random port on 127.0.0.1 (and the same port on ::1 when available,
    /// because browsers may resolve `localhost` to either).
    pub async fn bind(provider: Provider) -> Result<Self, OAuthError> {
        let v4 = TcpListener::bind("127.0.0.1:0").await.map_err(|e| OAuthError::Other(e.to_string()))?;
        let port = v4.local_addr().map_err(|e| OAuthError::Other(e.to_string()))?.port();
        let mut listeners = vec![v4];
        if provider.redirect_host() == "localhost" {
            if let Ok(v6) = TcpListener::bind(("::1", port)).await {
                listeners.push(v6);
            }
        }
        Ok(Loopback { listeners, redirect_uri: format!("http://{}:{port}/", provider.redirect_host()) })
    }

    /// Wait for the redirect carrying `state`; other requests get a 404.
    pub async fn wait(self, state: &str, timeout: Duration) -> Result<String, OAuthError> {
        let work = async {
            loop {
                let accepts = self.listeners.iter().map(|l| Box::pin(l.accept()));
                let (accepted, _, _) = futures::future::select_all(accepts).await;
                let Ok((mut stream, _)) = accepted else { continue };
                let mut buf = vec![0u8; 8192];
                let mut len = 0;
                // Read until the end of the request line (headers are not needed).
                loop {
                    let n = match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf[len..])).await {
                        Ok(Ok(n)) => n,
                        _ => 0,
                    };
                    len += n;
                    if n == 0 || buf[..len].windows(2).any(|w| w == b"\r\n") || len == buf.len() {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&buf[..len]);
                let line = text.lines().next().unwrap_or_default();
                let outcome = parse_redirect(line, state);
                let (status, body) = match outcome {
                    RedirectOutcome::Ignore => ("404 Not Found", "Not Found"),
                    _ => ("200 OK", DONE_PAGE),
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
                match outcome {
                    RedirectOutcome::Ignore => continue,
                    RedirectOutcome::Code(code) => return Ok(code),
                    RedirectOutcome::Error(e) => return Err(OAuthError::Denied(e)),
                }
            }
        };
        tokio::time::timeout(timeout, work).await.map_err(|_| OAuthError::TimedOut)?
    }
}

/// A token endpoint response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenResponse {
    pub access_token: String,
    /// Present on the first exchange; on refresh only when the provider rotates it.
    pub refresh_token: Option<String>,
    /// Lifetime in seconds.
    pub expires_in: i64,
}

#[derive(Deserialize)]
struct RawToken {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    error: Option<String>,
    error_description: Option<String>,
}

pub fn parse_token_response(body: &[u8]) -> Result<TokenResponse, OAuthError> {
    let raw: RawToken = serde_json::from_slice(body).map_err(|e| OAuthError::Other(format!("トークン応答を読めません: {e}")))?;
    if let Some(error) = raw.error {
        let detail = raw.error_description.map_or(error.clone(), |d| format!("{error}: {d}"));
        return Err(if error == "invalid_grant" { OAuthError::InvalidGrant(detail) } else { OAuthError::Other(detail) });
    }
    let access_token = raw.access_token.filter(|t| !t.is_empty()).ok_or_else(|| OAuthError::Other("アクセス トークンがありません".into()))?;
    Ok(TokenResponse { access_token, refresh_token: raw.refresh_token.filter(|t| !t.is_empty()), expires_in: raw.expires_in.unwrap_or(3600) })
}

fn http() -> Result<&'static reqwest::Client, OAuthError> {
    static CLIENT: std::sync::OnceLock<Option<reqwest::Client>> = std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            let tls = crate::tls::client_config().ok()?;
            reqwest::Client::builder()
                .use_preconfigured_tls((*tls).clone())
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(30))
                .build()
                .ok()
        })
        .as_ref()
        .ok_or_else(|| OAuthError::Other("HTTPS クライアントを初期化できません".into()))
}

async fn token_request(provider: Provider, pairs: &[(&str, &str)]) -> Result<TokenResponse, OAuthError> {
    let response = http()?
        .post(provider.token_endpoint())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .body(form(pairs))
        .send()
        .await
        .map_err(|e| OAuthError::Other(format!("{} に接続できません: {e}", provider.name())))?;
    let body = response.bytes().await.map_err(|e| OAuthError::Other(e.to_string()))?;
    parse_token_response(&body)
}

fn with_secret<'a>(client: &'a ClientConfig, mut pairs: Vec<(&'a str, &'a str)>) -> Vec<(&'a str, &'a str)> {
    if let Some(secret) = client.client_secret.as_deref().filter(|s| !s.is_empty()) {
        pairs.push(("client_secret", secret));
    }
    pairs
}

/// Exchange the authorisation code for tokens.
pub async fn exchange_code(provider: Provider, client: &ClientConfig, code: &str, verifier: &str, redirect_uri: &str) -> Result<TokenResponse, OAuthError> {
    let pairs = with_secret(
        client,
        vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", client.client_id.as_str()),
            ("redirect_uri", redirect_uri),
            ("code_verifier", verifier),
        ],
    );
    token_request(provider, &pairs).await
}

/// Get a fresh access token from a refresh token.
pub async fn refresh(provider: Provider, client: &ClientConfig, refresh_token: &str) -> Result<TokenResponse, OAuthError> {
    let mut pairs = with_secret(
        client,
        vec![("grant_type", "refresh_token"), ("refresh_token", refresh_token), ("client_id", client.client_id.as_str())],
    );
    if provider == Provider::Microsoft {
        pairs.push(("scope", provider.scopes()));
    }
    token_request(provider, &pairs).await
}

/// SASL XOAUTH2 initial response (before base64).
pub fn xoauth2(user: &str, access_token: &str) -> String {
    format!("user={user}\x01auth=Bearer {access_token}\x01\x01")
}

/// Whether a cached access token (expiry as Unix seconds) needs refreshing.
pub fn needs_refresh(expires_at: i64, now: i64) -> bool {
    expires_at - now < 300
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc7636_vector() {
        assert_eq!(pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        let a = random_token(32);
        assert_eq!(a.len(), 43);
        assert_ne!(a, random_token(32));
    }

    #[test]
    fn authorise_url_is_encoded() {
        let client = ClientConfig { client_id: "id.apps".into(), client_secret: None };
        let url = authorise_url(Provider::Google, &client, "http://127.0.0.1:5000/", "st", "ch", "a@gmail.com");
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?response_type=code&client_id=id.apps"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A5000%2F"));
        assert!(url.contains("scope=https%3A%2F%2Fmail.google.com%2F"));
        assert!(url.contains("code_challenge_method=S256") && url.contains("login_hint=a%40gmail.com"));
        assert!(url.contains("access_type=offline&prompt=consent"));
        let ms = authorise_url(Provider::Microsoft, &client, "http://localhost:5000/", "st", "ch", "");
        assert!(ms.contains("offline_access") && !ms.contains("login_hint") && !ms.contains("prompt="));
    }

    #[test]
    fn redirect_parsing() {
        assert_eq!(parse_redirect("GET /?code=4%2F0Ab&state=xyz&scope=s HTTP/1.1", "xyz"), RedirectOutcome::Code("4/0Ab".into()));
        assert_eq!(parse_redirect("GET /?code=abc&state=other HTTP/1.1", "xyz"), RedirectOutcome::Ignore);
        assert_eq!(parse_redirect("GET /favicon.ico HTTP/1.1", "xyz"), RedirectOutcome::Ignore);
        assert_eq!(parse_redirect("POST /?code=abc&state=xyz HTTP/1.1", "xyz"), RedirectOutcome::Ignore);
        assert_eq!(
            parse_redirect("GET /?error=access_denied&error_description=user+cancelled&state=xyz HTTP/1.1", "xyz"),
            RedirectOutcome::Error("access_denied: user cancelled".into())
        );
    }

    #[test]
    fn token_responses() {
        let first = parse_token_response(br#"{"access_token":"at","refresh_token":"rt","expires_in":3599,"token_type":"Bearer"}"#).unwrap();
        assert_eq!(first, TokenResponse { access_token: "at".into(), refresh_token: Some("rt".into()), expires_in: 3599 });
        let refreshed = parse_token_response(br#"{"access_token":"at2","expires_in":3600}"#).unwrap();
        assert_eq!(refreshed.refresh_token, None);
        assert!(matches!(
            parse_token_response(br#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#),
            Err(OAuthError::InvalidGrant(_))
        ));
        assert!(matches!(parse_token_response(br#"{"error":"invalid_client"}"#), Err(OAuthError::Other(_))));
    }

    #[test]
    fn xoauth2_and_expiry() {
        assert_eq!(xoauth2("a@b.jp", "tok"), "user=a@b.jp\u{1}auth=Bearer tok\u{1}\u{1}");
        assert!(needs_refresh(1000, 800));
        assert!(!needs_refresh(5000, 800));
    }

    #[test]
    fn clients_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CLIENTS_FILE);
        std::fs::write(&path, "[google]\nclient_id = \"g.apps\"\nclient_secret = \"s\"\n[microsoft]\nclient_id = \"\"\n").unwrap();
        let c = OAuthClients::load(&path);
        assert_eq!(c.get(Provider::Google).unwrap().client_secret.as_deref(), Some("s"));
        assert!(c.get(Provider::Microsoft).is_none(), "empty id counts as missing");
    }

    #[tokio::test]
    async fn loopback_receives_code() {
        let lb = Loopback::bind(Provider::Google).await.unwrap();
        let uri = lb.redirect_uri.clone();
        let port: u16 = uri.trim_start_matches("http://127.0.0.1:").trim_end_matches('/').parse().unwrap();
        let waiter = tokio::spawn(async move { lb.wait("st", Duration::from_secs(5)).await });
        for target in ["/favicon.ico", "/?code=good&state=st"] {
            let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            s.write_all(format!("GET {target} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes()).await.unwrap();
            let mut reply = String::new();
            s.read_to_string(&mut reply).await.unwrap();
            assert!(!reply.contains("good"), "code is not echoed");
        }
        assert_eq!(waiter.await.unwrap(), Ok("good".into()));
    }
}
