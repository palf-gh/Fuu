//! IMAP access: connect (implicit TLS or enforced STARTTLS, OS trust store), log
//! in, list folders and fetch message summaries.

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use async_imap::types::{Fetch, Flag, Name, NameAttribute};
use async_imap::{Client, Session};
use futures::TryStreamExt;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

use crate::account::{Security, ServerConfig};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImapError {
    Connect(String),
    Tls(String),
    Auth(String),
    Protocol(String),
    Timeout,
}

impl std::fmt::Display for ImapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect(e) => write!(f, "サーバーに接続できません: {e}"),
            Self::Tls(e) => write!(f, "暗号化接続を確立できません: {e}"),
            Self::Auth(e) => write!(f, "ログインできません（ユーザー名かパスワードを確認してください）: {e}"),
            Self::Protocol(e) => write!(f, "IMAP サーバーとの通信に失敗しました: {e}"),
            Self::Timeout => f.write_str("サーバーの応答がありません（タイムアウト）"),
        }
    }
}

impl std::error::Error for ImapError {}

fn protocol(e: impl std::fmt::Display) -> ImapError {
    ImapError::Protocol(e.to_string())
}

async fn timed<T>(limit: Duration, fut: impl std::future::Future<Output = Result<T, ImapError>>) -> Result<T, ImapError> {
    tokio::time::timeout(limit, fut).await.map_err(|_| ImapError::Timeout)?
}

/// The connection under the IMAP client.
#[derive(Debug)]
pub enum Stream {
    Tls(Box<TlsStream<TcpStream>>),
    #[cfg(feature = "insecure-test-transport")]
    Plain(TcpStream),
}

impl AsyncRead for Stream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
            #[cfg(feature = "insecure-test-transport")]
            Stream::Plain(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
            #[cfg(feature = "insecure-test-transport")]
            Stream::Plain(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
            #[cfg(feature = "insecure-test-transport")]
            Stream::Plain(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
            #[cfg(feature = "insecure-test-transport")]
            Stream::Plain(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}

pub type ImapSession = Session<Stream>;

/// TLS connector verifying certificates (OS trust store, Mozilla roots fallback).
fn tls_connector() -> Result<TlsConnector, ImapError> {
    crate::tls::client_config().map(TlsConnector::from).map_err(ImapError::Tls)
}

async fn tcp(server: &ServerConfig) -> Result<TcpStream, ImapError> {
    timed(CONNECT_TIMEOUT, async {
        TcpStream::connect((server.host.as_str(), server.port))
            .await
            .map_err(|e| ImapError::Connect(e.to_string()))
    })
    .await
}

async fn tls_upgrade(server: &ServerConfig, tcp: TcpStream) -> Result<Stream, ImapError> {
    let name = ServerName::try_from(server.host.clone()).map_err(|e| ImapError::Tls(e.to_string()))?;
    let tls = timed(CONNECT_TIMEOUT, async {
        tls_connector()?.connect(name, tcp).await.map_err(|e| ImapError::Tls(e.to_string()))
    })
    .await?;
    Ok(Stream::Tls(Box::new(tls)))
}

/// Open a connection and read the server greeting.
pub async fn connect(server: &ServerConfig) -> Result<Client<Stream>, ImapError> {
    match server.security {
        Security::Tls => {
            let stream = tls_upgrade(server, tcp(server).await?).await?;
            let mut client = Client::new(stream);
            timed(COMMAND_TIMEOUT, async { client.read_response().await.map_err(protocol) }).await?;
            Ok(client)
        }
        Security::StartTls => {
            let mut plain = Client::new(tcp(server).await?);
            timed(COMMAND_TIMEOUT, async { plain.read_response().await.map_err(protocol) }).await?;
            timed(COMMAND_TIMEOUT, async {
                plain.run_command_and_check_ok("STARTTLS", None).await.map_err(|e| ImapError::Tls(format!("STARTTLS: {e}")))
            })
            .await?;
            let stream = tls_upgrade(server, plain.into_inner()).await?;
            Ok(Client::new(stream))
        }
        #[cfg(feature = "insecure-test-transport")]
        Security::InsecurePlainForTests => {
            let mut client = Client::new(Stream::Plain(tcp(server).await?));
            timed(COMMAND_TIMEOUT, async { client.read_response().await.map_err(protocol) }).await?;
            Ok(client)
        }
    }
}

pub async fn login(server: &ServerConfig, username: &str, password: &str) -> Result<ImapSession, ImapError> {
    let client = connect(server).await?;
    timed(COMMAND_TIMEOUT, async {
        client.login(username, password).await.map_err(|(e, _)| ImapError::Auth(e.to_string()))
    })
    .await
}

/// What a folder is for, from SPECIAL-USE attributes or well-known names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FolderRole {
    Inbox,
    Drafts,
    Sent,
    Archive,
    Junk,
    Trash,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    /// Raw (modified UTF-7) path used in IMAP commands.
    pub path: String,
    /// Human-readable, decoded path.
    pub display: String,
    pub role: FolderRole,
    pub selectable: bool,
}

fn role_from_name(display: &str) -> FolderRole {
    let leaf = display.rsplit(['/', '.']).next().unwrap_or(display).to_lowercase();
    match leaf.as_str() {
        "inbox" => FolderRole::Inbox,
        "drafts" | "draft" | "下書き" => FolderRole::Drafts,
        "sent" | "sent items" | "sent messages" | "sent mail" | "送信済み" | "送信済みメール" | "送信済みアイテム" => FolderRole::Sent,
        "archive" | "archives" | "all mail" | "すべてのメール" | "アーカイブ" => FolderRole::Archive,
        "junk" | "spam" | "junk e-mail" | "junk email" | "迷惑メール" => FolderRole::Junk,
        "trash" | "deleted items" | "deleted messages" | "bin" | "ゴミ箱" | "ごみ箱" | "削除済みアイテム" => FolderRole::Trash,
        _ => FolderRole::Other,
    }
}

pub fn folder_from_parts(path: &str, attributes: &[NameAttribute<'_>]) -> Folder {
    let display = utf7_imap::decode_utf7_imap(path.to_owned());
    let mut role = None;
    let mut selectable = true;
    for attribute in attributes {
        match attribute {
            NameAttribute::NoSelect => selectable = false,
            NameAttribute::Drafts => role = Some(FolderRole::Drafts),
            NameAttribute::Sent => role = Some(FolderRole::Sent),
            NameAttribute::Archive | NameAttribute::All => role = role.or(Some(FolderRole::Archive)),
            NameAttribute::Junk => role = Some(FolderRole::Junk),
            NameAttribute::Trash => role = Some(FolderRole::Trash),
            _ => {}
        }
    }
    let role = if path.eq_ignore_ascii_case("INBOX") { FolderRole::Inbox } else { role.unwrap_or_else(|| role_from_name(&display)) };
    Folder { path: path.to_owned(), display, role, selectable }
}

fn folder_from_name(name: &Name) -> Folder {
    folder_from_parts(name.name(), name.attributes())
}

/// Inbox first, then the special folders, then everything else by name.
pub fn sort_folders(folders: &mut [Folder]) {
    folders.sort_by(|a, b| a.role.cmp(&b.role).then_with(|| a.display.to_lowercase().cmp(&b.display.to_lowercase())));
}

pub async fn list_folders(session: &mut ImapSession) -> Result<Vec<Folder>, ImapError> {
    let mut folders: Vec<Folder> = timed(COMMAND_TIMEOUT, async {
        let names: Vec<Name> = session.list(Some(""), Some("*")).await.map_err(protocol)?.try_collect().await.map_err(protocol)?;
        Ok(names.iter().map(folder_from_name).collect())
    })
    .await?;
    sort_folders(&mut folders);
    Ok(folders)
}

/// One row of the message list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageSummary {
    pub uid: u32,
    pub subject: String,
    pub from_name: String,
    pub from_email: String,
    /// Seconds since the Unix epoch (server INTERNALDATE).
    pub date: i64,
    pub seen: bool,
    pub answered: bool,
    pub flagged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FolderPage {
    pub uid_validity: Option<u32>,
    pub exists: u32,
    /// Newest first.
    pub messages: Vec<MessageSummary>,
}

/// Decode a header value (lenient RFC 2047, see `fuu_parser::headers`).
pub fn decode_header(raw: &[u8]) -> String {
    fuu_parser::headers::decode_header(raw)
}

fn summary_from_fetch(fetch: &Fetch) -> Option<MessageSummary> {
    let uid = fetch.uid?;
    let envelope = fetch.envelope();
    let subject = envelope.and_then(|e| e.subject.as_deref()).map(decode_header).unwrap_or_default();
    let from = envelope.and_then(|e| e.from.as_ref()).and_then(|list| list.first());
    let from_name = from.and_then(|a| a.name.as_deref()).map(decode_header).unwrap_or_default();
    let from_email = from
        .map(|a| {
            let mailbox = a.mailbox.as_deref().map(String::from_utf8_lossy).unwrap_or_default();
            let host = a.host.as_deref().map(String::from_utf8_lossy).unwrap_or_default();
            if host.is_empty() { mailbox.into_owned() } else { format!("{mailbox}@{host}") }
        })
        .unwrap_or_default();
    let flags: Vec<Flag<'_>> = fetch.flags().collect();
    Some(MessageSummary {
        uid,
        subject,
        from_name,
        from_email,
        date: fetch.internal_date().map(|d| d.timestamp()).unwrap_or_default(),
        seen: flags.contains(&Flag::Seen),
        answered: flags.contains(&Flag::Answered),
        flagged: flags.contains(&Flag::Flagged),
    })
}

/// Fetch the newest `limit` summaries of a folder. Uses EXAMINE (read-only), so
/// nothing is marked as read.
pub async fn fetch_summaries(session: &mut ImapSession, path: &str, limit: u32) -> Result<FolderPage, ImapError> {
    timed(COMMAND_TIMEOUT, async {
        let mailbox = session.examine(path).await.map_err(protocol)?;
        let mut page = FolderPage { uid_validity: mailbox.uid_validity, exists: mailbox.exists, messages: Vec::new() };
        if mailbox.exists == 0 {
            return Ok(page);
        }
        let start = mailbox.exists.saturating_sub(limit.max(1)) + 1;
        let fetches: Vec<Fetch> = session
            .fetch(format!("{start}:*"), "(UID FLAGS INTERNALDATE ENVELOPE)")
            .await
            .map_err(protocol)?
            .try_collect()
            .await
            .map_err(protocol)?;
        page.messages = fetches.iter().filter_map(summary_from_fetch).collect();
        page.messages.sort_by(|a, b| b.date.cmp(&a.date).then(b.uid.cmp(&a.uid)));
        Ok(page)
    })
    .await
}


/// Fetch the full raw message (without setting \\Seen), then mark it as read.
/// Returns the folder's UIDVALIDITY with the bytes.
pub async fn fetch_body(session: &mut ImapSession, path: &str, uid: u32) -> Result<(u32, Vec<u8>), ImapError> {
    timed(COMMAND_TIMEOUT, async {
        let mailbox = session.select(path).await.map_err(protocol)?;
        let fetches: Vec<Fetch> = session
            .uid_fetch(uid.to_string(), "(UID BODY.PEEK[])")
            .await
            .map_err(protocol)?
            .try_collect()
            .await
            .map_err(protocol)?;
        let raw = fetches
            .iter()
            .find(|f| f.uid == Some(uid))
            .or(fetches.first())
            .and_then(|f| f.body())
            .map(<[u8]>::to_vec)
            .ok_or_else(|| ImapError::Protocol(format!("UID {uid} のメールが見つかりません")))?;
        let _: Vec<Fetch> = session
            .uid_store(uid.to_string(), "+FLAGS.SILENT (\\Seen)")
            .await
            .map_err(protocol)?
            .try_collect()
            .await
            .map_err(protocol)?;
        Ok((mailbox.uid_validity.unwrap_or_default(), raw))
    })
    .await
}

/// Move a message to another folder: UID MOVE when supported, otherwise
/// COPY + \\Deleted + (UID) EXPUNGE.
pub async fn move_message(session: &mut ImapSession, from: &str, uid: u32, to: &str) -> Result<(), ImapError> {
    timed(COMMAND_TIMEOUT, async {
        let capabilities = session.capabilities().await.map_err(protocol)?;
        let can_move = capabilities.has_str("MOVE");
        let uidplus = capabilities.has_str("UIDPLUS");
        drop(capabilities);
        session.select(from).await.map_err(protocol)?;
        if can_move {
            return session.uid_mv(uid.to_string(), to).await.map_err(protocol);
        }
        session.uid_copy(uid.to_string(), to).await.map_err(protocol)?;
        let _: Vec<Fetch> = session
            .uid_store(uid.to_string(), "+FLAGS.SILENT (\\Deleted)")
            .await
            .map_err(protocol)?
            .try_collect()
            .await
            .map_err(protocol)?;
        if uidplus {
            let _: Vec<_> = session.uid_expunge(uid.to_string()).await.map_err(protocol)?.try_collect().await.map_err(protocol)?;
        } else {
            let _: Vec<_> = session.expunge().await.map_err(protocol)?.try_collect().await.map_err(protocol)?;
        }
        Ok(())
    })
    .await
}

/// Append a sent message to a folder, already marked as read.
pub async fn append_message(session: &mut ImapSession, path: &str, raw: &[u8]) -> Result<(), ImapError> {
    timed(COMMAND_TIMEOUT, async { session.append(path, Some("(\\Seen)"), None, raw).await.map_err(protocol) }).await
}

/// Mark a message as answered.
pub async fn mark_answered(session: &mut ImapSession, path: &str, uid: u32) -> Result<(), ImapError> {
    timed(COMMAND_TIMEOUT, async {
        session.select(path).await.map_err(protocol)?;
        let _: Vec<Fetch> = session
            .uid_store(uid.to_string(), "+FLAGS.SILENT (\\Answered)")
            .await
            .map_err(protocol)?
            .try_collect()
            .await
            .map_err(protocol)?;
        Ok(())
    })
    .await
}

/// Gmail stores sent mail itself; appending would create duplicates.
pub fn saves_sent_automatically(imap_host: &str) -> bool {
    let host = imap_host.to_ascii_lowercase();
    host.ends_with("gmail.com") || host.ends_with("googlemail.com")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_encoded_words_including_iso_2022_jp() {
        assert_eq!(decode_header(b"=?UTF-8?B?44GT44KT44Gr44Gh44Gv?="), "こんにちは");
        // "テスト" in ISO-2022-JP, base64
        assert_eq!(decode_header(b"=?ISO-2022-JP?B?GyRCJUYlOSVIGyhC?="), "テスト");
        assert_eq!(decode_header(b"Re: =?UTF-8?Q?caf=C3=A9?= menu"), "Re: café menu");
        assert_eq!(decode_header(b"plain\r\n folded"), "plain folded");
        assert_eq!(decode_header("生のUTF-8".as_bytes()), "生のUTF-8");
    }

    #[test]
    fn folder_roles_from_attributes_and_names() {
        assert_eq!(folder_from_parts("INBOX", &[]).role, FolderRole::Inbox);
        assert_eq!(folder_from_parts("Whatever", &[NameAttribute::Sent]).role, FolderRole::Sent);
        assert_eq!(folder_from_parts("Deleted Items", &[]).role, FolderRole::Trash);
        // Gmail-style modified UTF-7 path for "[Gmail]/送信済みメール"
        let raw = utf7_imap::encode_utf7_imap("[Gmail]/送信済みメール".to_owned());
        assert!(raw.is_ascii() && raw.contains('&'), "{raw}");
        let f = folder_from_parts(&raw, &[]);
        assert_eq!(f.display, "[Gmail]/送信済みメール");
        assert_eq!(f.role, FolderRole::Sent);
        let noselect = folder_from_parts("[Gmail]", &[NameAttribute::NoSelect]);
        assert!(!noselect.selectable);
    }

    #[test]
    fn folders_sort_inbox_first() {
        let mut v = vec![
            folder_from_parts("Zeta", &[]),
            folder_from_parts("Trash", &[]),
            folder_from_parts("INBOX", &[]),
            folder_from_parts("Alpha", &[]),
            folder_from_parts("Sent", &[]),
        ];
        sort_folders(&mut v);
        let order: Vec<&str> = v.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(order, ["INBOX", "Sent", "Trash", "Alpha", "Zeta"]);
    }
}
