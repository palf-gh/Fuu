//! End-to-end IMAP test against a local GreenMail server (never a real account).
//!
//! ```sh
//! docker run -d --name fuu-greenmail -p 3025:3025 -p 3143:3143 \
//!   -e GREENMAIL_OPTS="-Dgreenmail.setup.test.all -Dgreenmail.hostname=0.0.0.0 -Dgreenmail.users=test:secret@localhost" \
//!   greenmail/standalone:2.1.3
//! FUU_GREENMAIL=1 cargo test -p fuu-core --features insecure-test-transport --test greenmail
//! ```
//!
//! GreenMail's test ports are unencrypted, hence the test-only feature.
#![cfg(feature = "insecure-test-transport")]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use fuu_core::account::{Account, AuthKind, Security, ServerConfig};
use fuu_core::engine::{Command, Engine, Event};
use fuu_core::imap::{self, FolderRole};
use fuu_core::secrets::{MemoryStore, SecretStore};

const USER: &str = "test";
const PASSWORD: &str = "secret";
const ADDRESS: &str = "test@localhost";

fn password(p: &str) -> imap::Credential {
    imap::Credential::Password(p.to_owned())
}

fn enabled() -> bool {
    std::env::var_os("FUU_GREENMAIL").is_some()
}

fn imap_server() -> ServerConfig {
    ServerConfig { host: "127.0.0.1".into(), port: 3143, security: Security::InsecurePlainForTests }
}

/// Minimal SMTP submission to seed the mailbox.
fn smtp_send(raw_message: &str) {
    let stream = TcpStream::connect(("127.0.0.1", 3025)).expect("GreenMail SMTP on 3025");
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut writer = stream;
    let mut expect = |code: &str| {
        let mut line = String::new();
        loop {
            line.clear();
            reader.read_line(&mut line).unwrap();
            if line.len() < 4 || line.as_bytes()[3] != b'-' {
                break;
            }
        }
        assert!(line.starts_with(code), "expected {code}, got {line:?}");
    };
    expect("220");
    for (cmd, code) in [
        ("EHLO fuu-test\r\n".to_owned(), "250"),
        (format!("MAIL FROM:<sender@example.jp>\r\n"), "250"),
        (format!("RCPT TO:<{ADDRESS}>\r\n"), "250"),
        ("DATA\r\n".to_owned(), "354"),
    ] {
        writer.write_all(cmd.as_bytes()).unwrap();
        expect(code);
    }
    writer.write_all(raw_message.replace('\n', "\r\n").as_bytes()).unwrap();
    writer.write_all(b"\r\n.\r\n").unwrap();
    expect("250");
    writer.write_all(b"QUIT\r\n").unwrap();
}

fn seed() {
    smtp_send("From: =?UTF-8?B?5bGx55SwIOWkqumDjg==?= <taro@example.jp>\nTo: test@localhost\nSubject: =?ISO-2022-JP?B?GyRCJUYlOSVIGyhC?=\nDate: Tue, 17 Nov 2026 10:00:00 +0900\nMIME-Version: 1.0\nContent-Type: text/plain; charset=UTF-8\n\nhello");
    smtp_send("From: Oliver Smith <oliver@example.co.uk>\nTo: test@localhost\nSubject: Colour proposal\nDate: Wed, 18 Nov 2026 09:00:00 +0000\n\nbody");
}

#[tokio::test]
async fn conversation_follows_references_across_inbox_and_sent() {
    if !enabled() {
        eprintln!("skipped: set FUU_GREENMAIL=1 with GreenMail running");
        return;
    }
    let mut session = imap::login(&imap_server(), USER, &password(PASSWORD)).await.unwrap();
    let _ = session.create("Sent").await;
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let root = format!("{nonce}-root@example.org");
    let reply = format!("{nonce}-reply@example.org");
    let subject = format!("Thread test {nonce}");
    let make = |id: &str, extra: &str| format!("From: a@example.org\r\nTo: test@localhost\r\nSubject: {subject}\r\nMessage-ID: <{id}>\r\n{extra}\r\n\r\nBody\r\n");
    imap::append_message(&mut session, "INBOX", make(&root, "").as_bytes()).await.unwrap();
    imap::append_message(&mut session, "Sent", make(&reply, &format!("In-Reply-To: <{root}>\r\nReferences: <{root}>")).as_bytes()).await.unwrap();
    let other = format!("{nonce}-unrelated@example.org");
    imap::append_message(&mut session, "INBOX", make(&other, "").as_bytes()).await.unwrap();
    let page = imap::fetch_summaries(&mut session, "INBOX", 20).await.unwrap();
    // The unrelated message has the same subject, so locate the root by its
    // append order: the root is the smaller of the two UIDs.
    let uid = page.messages.iter().filter(|m| m.subject == subject).map(|m| m.uid).min().unwrap();
    let thread = imap::fetch_conversation(&mut session, "INBOX", Some("Sent"), uid).await.unwrap();
    assert_eq!(thread.len(), 2);
    assert!(thread.iter().any(|m| m.folder == "INBOX" && m.summary.uid == uid));
    assert!(thread.iter().any(|m| m.folder == "Sent"));
}

#[tokio::test]
async fn lists_folders_and_fetches_summaries() {
    if !enabled() {
        eprintln!("skipped: set FUU_GREENMAIL=1 with GreenMail running");
        return;
    }
    seed();
    tokio::time::sleep(Duration::from_millis(500)).await;

    let wrong = imap::login(&imap_server(), USER, &password("wrong")).await;
    assert!(matches!(wrong, Err(imap::ImapError::Auth(_))), "{wrong:?}");

    let mut session = imap::login(&imap_server(), USER, &password(PASSWORD)).await.expect("login");
    let folders = imap::list_folders(&mut session).await.expect("list");
    assert_eq!(folders.first().map(|f| f.role), Some(FolderRole::Inbox));

    let page = imap::fetch_summaries(&mut session, "INBOX", 50).await.expect("fetch");
    assert!(page.exists >= 2);
    let newest = &page.messages[0];
    assert!(page.messages.windows(2).all(|w| w[0].date >= w[1].date), "newest first");
    let japanese = page.messages.iter().find(|m| m.subject == "テスト").expect("ISO-2022-JP subject decoded");
    assert_eq!(japanese.from_name, "山田 太郎");
    assert_eq!(japanese.from_email, "taro@example.jp");
    assert!(!japanese.seen, "EXAMINE must not mark as read");
    assert!(newest.uid > 0);
    session.logout().await.ok();
}

#[test]
fn engine_adds_account_and_loads_inbox() {
    if !enabled() {
        eprintln!("skipped: set FUU_GREENMAIL=1 with GreenMail running");
        return;
    }
    seed();
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("accounts.toml");
    let secrets = Arc::new(MemoryStore::default());
    let (tx, rx) = mpsc::channel();
    let engine = Engine::start(config.clone(), Some(dir.path().join("cache.sqlite")), secrets.clone(), move |event| {
        let _ = tx.send(event);
    });
    engine.send(Command::LoadAccounts);
    let account = Account {
        id: String::new(),
        display_name: "Test".into(),
        email: ADDRESS.into(),
        username: USER.into(),
        imap: imap_server(),
        smtp: ServerConfig { host: "127.0.0.1".into(), port: 3025, security: Security::InsecurePlainForTests },
        auth: AuthKind::Password,
        oauth: None,
        signature: String::new(),
        signature_enabled: true,
    };

    // A wrong password must not create the account.
    engine.send(Command::AddAccount { account: account.clone(), password: "wrong".into() });
    let failed = wait_for(&rx, |e| matches!(e, Event::AccountAddFailed(_)));
    assert!(matches!(failed, Event::AccountAddFailed(_)));
    assert!(fuu_core::config::load(&config).unwrap().accounts.is_empty());

    engine.send(Command::AddAccount { account, password: PASSWORD.into() });
    let Event::AccountAdded(added) = wait_for(&rx, |e| matches!(e, Event::AccountAdded(_))) else { unreachable!() };
    assert_eq!(secrets.get(&added.id).unwrap(), PASSWORD);
    assert_eq!(fuu_core::config::load(&config).unwrap().accounts.len(), 1);
    let Event::Messages { page, path, .. } = wait_for(&rx, |e| matches!(e, Event::Messages { .. })) else { unreachable!() };
    assert_eq!(path, "INBOX");
    assert!(page.messages.iter().any(|m| m.subject == "Colour proposal"));

    // Body: fetched, parsed, marked as read on the server, then served from the cache.
    let target = page.messages.iter().find(|m| m.subject == "Colour proposal").unwrap().clone();
    engine.send(Command::FetchBody { account_id: added.id.clone(), folder: "INBOX".into(), uid: target.uid });
    let Event::Body { raw, .. } = wait_for(&rx, |e| matches!(e, Event::Body { .. })) else { unreachable!() };
    let parsed = fuu_parser::MessageParser::parse(&fuu_parser::mailparser::MailParserEngine, &raw, &Default::default()).unwrap();
    assert_eq!(parsed.headers.subject, "Colour proposal");
    engine.send(Command::OpenFolder { account_id: added.id.clone(), path: "INBOX".into() });
    let Event::Messages { page, .. } = wait_for(&rx, |e| matches!(e, Event::Messages { .. })) else { unreachable!() };
    assert!(page.messages.iter().find(|m| m.uid == target.uid).unwrap().seen, "opening marks as read");
    engine.send(Command::FetchBody { account_id: added.id.clone(), folder: "INBOX".into(), uid: target.uid });
    let Event::Body { raw: cached, .. } = wait_for(&rx, |e| matches!(e, Event::Body { .. })) else { unreachable!() };
    assert_eq!(cached, raw);

    engine.send(Command::RemoveAccount { account_id: added.id.clone() });
    wait_for(&rx, |e| matches!(e, Event::Accounts(list) if list.is_empty()));
    assert!(secrets.get(&added.id).is_err(), "secret removed with the account");
}

#[test]
fn engine_sends_reply_saves_sent_and_marks_answered() {
    if !enabled() {
        eprintln!("skipped: set FUU_GREENMAIL=1 with GreenMail running");
        return;
    }
    seed();
    // GreenMail has no Sent folder by default.
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut s = imap::login(&imap_server(), USER, &password(PASSWORD)).await.unwrap();
        let _ = s.create("Sent").await;
        s.logout().await.ok();
    });

    let dir = tempfile::tempdir().unwrap();
    let secrets = Arc::new(MemoryStore::default());
    let (tx, rx) = mpsc::channel();
    let engine = Engine::start(dir.path().join("accounts.toml"), None, secrets.clone(), move |e| {
        let _ = tx.send(e);
    });
    engine.send(Command::LoadAccounts);
    let account = Account {
        id: String::new(),
        display_name: "テスト".into(),
        email: ADDRESS.into(),
        username: USER.into(),
        imap: imap_server(),
        smtp: ServerConfig { host: "127.0.0.1".into(), port: 3025, security: Security::InsecurePlainForTests },
        auth: AuthKind::Password,
        oauth: None,
        signature: String::new(),
        signature_enabled: true,
    };
    engine.send(Command::AddAccount { account, password: PASSWORD.into() });
    let Event::AccountAdded(added) = wait_for(&rx, |e| matches!(e, Event::AccountAdded(_))) else { unreachable!() };
    let Event::Messages { page, .. } = wait_for(&rx, |e| matches!(e, Event::Messages { .. })) else { unreachable!() };
    let original = page.messages.iter().find(|m| m.subject == "Colour proposal").unwrap().clone();

    let subject = format!("Re: 返信テスト {}", std::process::id());
    let outgoing = fuu_core::compose::Outgoing {
        from_name: "テスト".into(),
        from_email: ADDRESS.into(),
        to: vec![fuu_core::compose::Recipient { name: None, email: ADDRESS.into() }],
        subject: subject.clone(),
        body: "本文\n\n-- \n署名".into(),
        in_reply_to: Some("orig@example.jp".into()),
        references: vec!["orig@example.jp".into()],
        html: true,
        ..Default::default()
    };
    engine.send(Command::Send { account_id: added.id.clone(), outgoing, answered: Some(("INBOX".into(), original.uid)), draft_id: 7 });
    let Event::Sent { draft_id, warning, .. } = wait_for(&rx, |e| matches!(e, Event::Sent { .. } | Event::SendFailed { .. })) else {
        panic!("send failed")
    };
    assert_eq!(draft_id, 7);
    assert_eq!(warning, None, "saved to Sent");

    std::thread::sleep(Duration::from_millis(500));
    engine.send(Command::OpenFolder { account_id: added.id.clone(), path: "INBOX".into() });
    let Event::Messages { page, .. } = wait_for(&rx, |e| matches!(e, Event::Messages { .. })) else { unreachable!() };
    assert!(page.messages.iter().any(|m| m.subject == subject), "delivered");
    assert!(page.messages.iter().find(|m| m.uid == original.uid).unwrap().answered, "original marked answered");
    engine.send(Command::OpenFolder { account_id: added.id.clone(), path: "Sent".into() });
    let Event::Messages { page, .. } = wait_for(&rx, |e| matches!(e, Event::Messages { .. })) else { unreachable!() };
    let sent = page.messages.iter().find(|m| m.subject == subject).expect("copy in Sent");
    assert!(sent.seen);
}

#[tokio::test]
async fn idle_reports_new_mail() {
    if !enabled() {
        eprintln!("skipped: set FUU_GREENMAIL=1 with GreenMail running");
        return;
    }
    let mut session = imap::login(&imap_server(), USER, &password(PASSWORD)).await.expect("login");
    assert!(imap::supports_idle(&mut session).await.unwrap(), "GreenMail supports IDLE");
    let before = imap::examine_state(&mut session, "INBOX").await.unwrap();

    // Quiet round: nothing arrives, the round ends at its limit.
    let (session, outcome) = imap::idle_round(session, Duration::from_millis(800)).await.expect("quiet round");
    assert_eq!(outcome, imap::IdleOutcome::Quiet);

    let deliver = tokio::task::spawn_blocking(|| {
        std::thread::sleep(Duration::from_millis(700));
        smtp_send("From: a@example.jp\nTo: test@localhost\nSubject: idle\n\nnew");
    });
    let started = std::time::Instant::now();
    let (mut session, outcome) = imap::idle_round(session, Duration::from_secs(30)).await.expect("idle round");
    deliver.await.unwrap();
    assert_eq!(outcome, imap::IdleOutcome::Changed);
    assert!(started.elapsed() < Duration::from_secs(15), "woken by the server, not the limit");
    let after = imap::examine_state(&mut session, "INBOX").await.unwrap();
    assert!(after.0 > before.0, "{before:?} -> {after:?}");
    session.logout().await.ok();
}

#[test]
fn engine_watcher_reports_inbox_changes() {
    if !enabled() {
        eprintln!("skipped: set FUU_GREENMAIL=1 with GreenMail running");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = mpsc::channel();
    let engine = Engine::start(dir.path().join("accounts.toml"), None, Arc::new(MemoryStore::default()), move |e| {
        let _ = tx.send(e);
    });
    engine.send(Command::LoadAccounts);
    let account = Account {
        id: String::new(),
        display_name: "Test".into(),
        email: ADDRESS.into(),
        username: USER.into(),
        imap: imap_server(),
        smtp: ServerConfig { host: "127.0.0.1".into(), port: 3025, security: Security::InsecurePlainForTests },
        auth: AuthKind::Password,
        oauth: None,
        signature: String::new(),
        signature_enabled: true,
    };
    engine.send(Command::AddAccount { account, password: PASSWORD.into() });
    let Event::AccountAdded(added) = wait_for(&rx, |e| matches!(e, Event::AccountAdded(_))) else { unreachable!() };
    wait_for(&rx, |e| matches!(e, Event::Messages { .. }));
    // Give the watcher time to log in and enter IDLE.
    std::thread::sleep(Duration::from_secs(2));
    smtp_send("From: b@example.jp\nTo: test@localhost\nSubject: watcher\n\nnew");
    let Event::FolderChanged { account_id, path } = wait_for(&rx, |e| matches!(e, Event::FolderChanged { .. })) else { unreachable!() };
    assert_eq!((account_id.as_str(), path.as_str()), (added.id.as_str(), "INBOX"));

    // A background refresh is marked as such and carries the new mail.
    engine.send(Command::RefreshFolder { account_id: added.id.clone(), path: "INBOX".into() });
    let Event::Messages { page, refresh, .. } = wait_for(&rx, |e| matches!(e, Event::Messages { .. })) else { unreachable!() };
    assert!(refresh);
    assert!(page.messages.iter().any(|m| m.subject == "watcher"));
    engine.send(Command::RemoveAccount { account_id: added.id });
    wait_for(&rx, |e| matches!(e, Event::Accounts(list) if list.is_empty()));
}

fn wait_for(rx: &mpsc::Receiver<Event>, pred: impl Fn(&Event) -> bool) -> Event {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let event = rx.recv_timeout(left).expect("event before timeout");
        if let Event::Error(e) = &event {
            panic!("engine error: {e}");
        }
        if let Event::SendFailed { message, .. } = &event {
            panic!("send failed: {message}");
        }
        if pred(&event) {
            return event;
        }
    }
}
