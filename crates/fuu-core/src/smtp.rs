//! Minimal SMTP submission client over Fuu's TLS configuration (implicit TLS or
//! enforced STARTTLS; AUTH PLAIN / LOGIN). Written in-house so SMTP uses exactly
//! the same certificate verification as IMAP.

use std::time::Duration;

use base64::Engine as _;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use crate::account::{Security, ServerConfig};
use crate::imap::Credential;
use fuu_i18n::tr;

const TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmtpError {
    Connect(String),
    Tls(String),
    Auth(String),
    Rejected(String),
    Protocol(String),
    Timeout,
}

impl std::fmt::Display for SmtpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect(e) => f.write_str(&tr!("Cannot connect to the outgoing server: {0}", e)),
            Self::Tls(e) => f.write_str(&tr!("Cannot set up an encrypted connection to the outgoing server: {0}", e)),
            Self::Auth(e) => f.write_str(&tr!("Cannot sign in to the outgoing server: {0}", e)),
            Self::Rejected(e) => f.write_str(&tr!("Sending was refused: {0}", e)),
            Self::Protocol(e) => f.write_str(&tr!("Talking to the outgoing server failed: {0}", e)),
            Self::Timeout => f.write_str(tr!("The outgoing server did not answer (timed out)")),
        }
    }
}

impl std::error::Error for SmtpError {}

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

struct Conn {
    stream: BufReader<Box<dyn Io>>,
}

#[derive(Debug)]
struct Reply {
    code: u16,
    lines: Vec<String>,
}

impl Reply {
    fn text(&self) -> String {
        format!("{} {}", self.code, self.lines.join(" / "))
    }
}

impl Conn {
    async fn read(&mut self) -> Result<Reply, SmtpError> {
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            let n = self.stream.read_line(&mut line).await.map_err(|e| SmtpError::Protocol(e.to_string()))?;
            if n == 0 {
                return Err(SmtpError::Protocol(tr!("The connection was closed").into()));
            }
            let line = line.trim_end();
            if line.len() < 3 {
                return Err(SmtpError::Protocol(tr!("Unexpected reply: {0}", line)));
            }
            let code: u16 = line[..3].parse().map_err(|_| SmtpError::Protocol(tr!("Unexpected reply: {0}", line)))?;
            let last = line.as_bytes().get(3) != Some(&b'-');
            lines.push(line.get(4..).unwrap_or("").to_owned());
            if last {
                return Ok(Reply { code, lines });
            }
        }
    }

    async fn command(&mut self, line: &str) -> Result<Reply, SmtpError> {
        self.stream.get_mut().write_all(format!("{line}\r\n").as_bytes()).await.map_err(|e| SmtpError::Protocol(e.to_string()))?;
        self.stream.get_mut().flush().await.map_err(|e| SmtpError::Protocol(e.to_string()))?;
        self.read().await
    }

    async fn expect(&mut self, line: &str, ok: impl Fn(u16) -> bool) -> Result<Reply, SmtpError> {
        let reply = self.command(line).await?;
        if ok(reply.code) { Ok(reply) } else { Err(SmtpError::Protocol(reply.text())) }
    }

    async fn ehlo(&mut self) -> Result<Vec<String>, SmtpError> {
        let reply = self.expect("EHLO localhost", |c| c == 250).await?;
        Ok(reply.lines.iter().map(|l| l.to_ascii_uppercase()).collect())
    }
}

async fn tls_wrap(server: &ServerConfig, tcp: Box<dyn Io>) -> Result<Box<dyn Io>, SmtpError> {
    let config = crate::tls::client_config().map_err(SmtpError::Tls)?;
    let name = ServerName::try_from(server.host.clone()).map_err(|e| SmtpError::Tls(e.to_string()))?;
    let tls = TlsConnector::from(config).connect(name, tcp).await.map_err(|e| SmtpError::Tls(e.to_string()))?;
    Ok(Box::new(tls))
}

async fn open(server: &ServerConfig) -> Result<(Conn, Vec<String>), SmtpError> {
    let tcp = TcpStream::connect((server.host.as_str(), server.port)).await.map_err(|e| SmtpError::Connect(e.to_string()))?;
    let plain: Box<dyn Io> = Box::new(tcp);
    match server.security {
        Security::Tls => {
            let mut conn = Conn { stream: BufReader::new(tls_wrap(server, plain).await?) };
            greeting(&mut conn).await?;
            let caps = conn.ehlo().await?;
            Ok((conn, caps))
        }
        Security::StartTls => {
            let mut conn = Conn { stream: BufReader::new(plain) };
            greeting(&mut conn).await?;
            let caps = conn.ehlo().await?;
            if !caps.iter().any(|c| c.starts_with("STARTTLS")) {
                return Err(SmtpError::Tls(tr!("The server does not support STARTTLS").into()));
            }
            conn.expect("STARTTLS", |c| c == 220).await.map_err(|e| SmtpError::Tls(e.to_string()))?;
            let inner = conn.stream.into_inner();
            let mut conn = Conn { stream: BufReader::new(tls_wrap(server, inner).await?) };
            let caps = conn.ehlo().await?;
            Ok((conn, caps))
        }
        #[cfg(feature = "insecure-test-transport")]
        Security::InsecurePlainForTests => {
            let mut conn = Conn { stream: BufReader::new(plain) };
            greeting(&mut conn).await?;
            let caps = conn.ehlo().await?;
            Ok((conn, caps))
        }
    }
}

async fn greeting(conn: &mut Conn) -> Result<(), SmtpError> {
    let reply = conn.read().await?;
    if reply.code == 220 { Ok(()) } else { Err(SmtpError::Connect(reply.text())) }
}

async fn authenticate(conn: &mut Conn, caps: &[String], username: &str, credential: &Credential) -> Result<(), SmtpError> {
    let auth = caps.iter().find(|c| c.starts_with("AUTH")).cloned().unwrap_or_default();
    let b64 = base64::engine::general_purpose::STANDARD;
    let password = match credential {
        Credential::Password(p) => p.as_str(),
        Credential::Bearer(token) => {
            let r = conn.command(&format!("AUTH XOAUTH2 {}", b64.encode(crate::oauth::xoauth2(username, token)))).await?;
            if r.code == 235 {
                return Ok(());
            }
            if r.code == 334 {
                // Error details arrive as a base64 JSON challenge; end the exchange.
                let detail = r
                    .lines
                    .first()
                    .and_then(|l| b64.decode(l.trim()).ok())
                    .map(|d| String::from_utf8_lossy(&d).into_owned())
                    .unwrap_or_default();
                let fin = conn.command("").await?;
                return Err(SmtpError::Auth(if detail.is_empty() { fin.text() } else { detail }));
            }
            return Err(SmtpError::Auth(r.text()));
        }
    };
    let reply = if auth.contains("PLAIN") || auth.is_empty() {
        conn.command(&format!("AUTH PLAIN {}", b64.encode(format!("\0{username}\0{password}")))).await?
    } else if auth.contains("LOGIN") {
        let r = conn.command("AUTH LOGIN").await?;
        if r.code != 334 {
            return Err(SmtpError::Auth(r.text()));
        }
        let r = conn.command(&b64.encode(username)).await?;
        if r.code != 334 {
            return Err(SmtpError::Auth(r.text()));
        }
        conn.command(&b64.encode(password)).await?
    } else {
        return Err(SmtpError::Auth(tr!("Unsupported sign-in method ({0})", auth)));
    };
    if reply.code == 235 { Ok(()) } else { Err(SmtpError::Auth(reply.text())) }
}

/// Dot-stuff and normalise line endings for DATA.
pub fn data_payload(raw: &[u8]) -> Vec<u8> {
    let text = raw.split(|&b| b == b'\n').map(|l| l.strip_suffix(b"\r").unwrap_or(l));
    let mut out = Vec::with_capacity(raw.len() + 64);
    for line in text {
        if line.first() == Some(&b'.') {
            out.push(b'.');
        }
        out.extend_from_slice(line);
        out.extend_from_slice(b"\r\n");
    }
    // `split` yields a trailing empty line when raw ends with \n; keep exactly one CRLF.
    while out.ends_with(b"\r\n\r\n") {
        out.truncate(out.len() - 2);
    }
    out.extend_from_slice(b".\r\n");
    out
}

/// Submit one message.
pub async fn send(server: &ServerConfig, username: &str, credential: &Credential, from: &str, recipients: &[String], raw: &[u8]) -> Result<(), SmtpError> {
    let job = async {
        let (mut conn, caps) = open(server).await?;
        authenticate(&mut conn, &caps, username, credential).await?;
        let r = conn.command(&format!("MAIL FROM:<{from}>")).await?;
        if r.code != 250 {
            return Err(SmtpError::Rejected(r.text()));
        }
        for rcpt in recipients {
            let r = conn.command(&format!("RCPT TO:<{rcpt}>")).await?;
            if r.code != 250 && r.code != 251 {
                return Err(SmtpError::Rejected(format!("{rcpt}: {}", r.text())));
            }
        }
        conn.expect("DATA", |c| c == 354).await?;
        let payload = data_payload(raw);
        conn.stream.get_mut().write_all(&payload).await.map_err(|e| SmtpError::Protocol(e.to_string()))?;
        conn.stream.get_mut().flush().await.map_err(|e| SmtpError::Protocol(e.to_string()))?;
        let r = conn.read().await?;
        if r.code != 250 {
            return Err(SmtpError::Rejected(r.text()));
        }
        let _ = conn.command("QUIT").await;
        Ok(())
    };
    tokio::time::timeout(TIMEOUT, job).await.map_err(|_| SmtpError::Timeout)?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_stuffs_and_terminates() {
        assert_eq!(data_payload(b"a\n.b\r\n..c\n"), b"a\r\n..b\r\n...c\r\n.\r\n");
        assert_eq!(data_payload(b"x"), b"x\r\n.\r\n");
    }
}
