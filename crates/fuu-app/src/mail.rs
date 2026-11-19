//! Bridge between the mail engine (fuu-core) and the UI: accounts, folders, the
//! message list, message bodies and the account-setup form. Everything here runs
//! on the UI thread.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use chrono::{Datelike, Local, TimeZone};
use fuu_core::account::{self, Account, AuthKind, Security, ServerConfig};
use fuu_core::engine::{Command, Engine, Event, MoveTarget};
use fuu_core::imap::{Folder, FolderRole, MessageSummary};
use fuu_core::secrets::KeychainStore;
use fuu_model::{Address, Attachment, ResourceManifest, SafeDocument};
use fuu_parser::mailparser::{self, MailParserEngine};
use fuu_parser::{MessageParser, ParseLimits};
use fuu_sanitiser::filename;
use fuu_sanitiser::escape_html;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::tabs::MessageRef;
use crate::{avatar_index, AccountItem, AppWindow, AttachmentItem, FolderItem, MessageRow};

#[derive(Default)]
pub struct AppData {
    /// Dummy data for UI development (`FUU_SPIKE_FEED=1`, snapshot runs).
    pub spike: bool,
    pub engine: Option<Arc<Engine>>,
    pub accounts: Vec<Account>,
    pub active: Option<String>,
    pub folders: Vec<Folder>,
    pub folder: String,
    pub summaries: Vec<MessageSummary>,
    /// Whether the setup form's server fields were filled automatically.
    setup_auto: bool,
    /// Parsed and sanitised bodies by (account, folder, uid).
    pub bodies: HashMap<(String, String, u32), Rc<Body>>,
    /// Bodies requested from the engine and not yet delivered.
    pending: Vec<(String, String, u32)>,
    /// Last body failure per message, shown instead of the body.
    failures: HashMap<(String, String, u32), String>,
    /// Documents with remote images the user allowed (per message, this session).
    allowed: HashMap<(String, String, u32), Rc<SafeDocument>>,
    remote_pending: Vec<(String, String, u32)>,
    /// Account whose OAuth sign-in must be renewed, and whether a renewal runs.
    reauth: Option<String>,
    reauth_running: bool,
}

/// What the reader shows about remote images for a message.
pub struct RemoteState {
    /// The document to display (with images when allowed).
    pub doc: SafeDocument,
    /// Some remote content was blocked.
    pub blocked: bool,
    /// Blocked images can be allowed (not only tracking pixels).
    pub allowable: bool,
    pub loading: bool,
}

/// The document to show for a loaded body, with remote images if allowed.
pub fn remote_state(r: &MessageRef, body: &Body) -> RemoteState {
    let k = key(r);
    if let Some(doc) = DATA.with(|d| d.borrow().allowed.get(&k).cloned()) {
        return RemoteState { doc: (*doc).clone(), blocked: false, allowable: false, loading: false };
    }
    let loading = DATA.with(|d| d.borrow().remote_pending.contains(&k));
    RemoteState {
        doc: body.doc.clone(),
        blocked: body.doc.flags.remote_images,
        allowable: !body.doc.remote_images.is_empty(),
        loading,
    }
}

/// Fetch the remote images of this message (the user pressed "show").
pub fn allow_remote(r: &MessageRef) {
    let k = key(r);
    let urls = DATA.with(|d| d.borrow().bodies.get(&k).map(|b| b.doc.remote_images.clone())).unwrap_or_default();
    if urls.is_empty() || DATA.with(|d| d.borrow().remote_pending.contains(&k)) {
        return;
    }
    DATA.with(|d| d.borrow_mut().remote_pending.push(k.clone()));
    send(Command::FetchRemoteImages { account_id: k.0, folder: k.1, uid: k.2, urls });
}

/// A message body ready for display.
pub struct Body {
    pub headers: fuu_model::Headers,
    pub doc: SafeDocument,
    pub attachments: Vec<Attachment>,
    pub raw: Arc<Vec<u8>>,
    pub to: String,
    pub cc: String,
}

fn key(r: &MessageRef) -> (String, String, u32) {
    (r.account_id.clone(), r.folder.clone(), r.uid)
}

fn addresses_text(list: &[Address]) -> String {
    list.iter()
        .map(|a| match &a.name {
            Some(n) => format!("{n} <{}>", a.email),
            None => a.email.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn process(raw: Arc<Vec<u8>>) -> Body {
    match MailParserEngine.parse(&raw, &ParseLimits::default()) {
        Ok(message) => Body {
            headers: message.headers.clone(),
            doc: fuu_sanitiser::sanitise(&message),
            attachments: message.attachments.clone(),
            to: addresses_text(&message.headers.to),
            cc: addresses_text(&message.headers.cc),
            raw,
        },
        Err(e) => Body {
            headers: fuu_model::Headers::default(),
            doc: notice_document(&format!("このメールは表示できません（{e}）。")),
            attachments: Vec::new(),
            to: String::new(),
            cc: String::new(),
            raw,
        },
    }
}

/// A quiet one-line document (loading, errors).
pub fn notice_document(text: &str) -> SafeDocument {
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><style>:root{{color-scheme:light dark}}body{{font:14px/1.7 system-ui,'Segoe UI','Hiragino Sans','Yu Gothic UI',sans-serif;margin:6px 40px 48px;opacity:.55}}</style></head><body><p>{}</p></body></html>",
        escape_html(text)
    );
    SafeDocument { html, text: text.to_owned(), resources: ResourceManifest::default(), flags: Default::default(), remote_images: Vec::new() }
}

/// The body for a message tab: cached, failed, or requested now (loading).
pub fn body_or_request(r: &MessageRef) -> Result<Rc<Body>, SafeDocument> {
    let k = key(r);
    if let Some(body) = DATA.with(|d| d.borrow().bodies.get(&k).cloned()) {
        return Ok(body);
    }
    if let Some(message) = DATA.with(|d| d.borrow().failures.get(&k).cloned()) {
        return Err(notice_document(&message));
    }
    let already = DATA.with(|d| d.borrow().pending.contains(&k));
    if !already {
        DATA.with(|d| d.borrow_mut().pending.push(k.clone()));
        send(Command::FetchBody { account_id: k.0, folder: k.1, uid: k.2 });
    }
    Err(notice_document("読み込み中…"))
}

pub fn attachment_items(body: &Body) -> Vec<AttachmentItem> {
    body.attachments
        .iter()
        .map(|a| AttachmentItem {
            name: filename::sanitise_file_name(&a.file_name).into(),
            size: human_size(a.size).into(),
            executable: filename::is_executable(&a.file_name),
        })
        .collect()
}

fn human_size(bytes: u64) -> String {
    match bytes {
        b if b < 1024 => format!("{b} B"),
        b if b < 1024 * 1024 => format!("{:.0} KB", b as f64 / 1024.0),
        b => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
    }
}

/// Ask where to save an attachment and write it, marked as downloaded from the
/// internet so the OS applies its usual protections.
pub fn save_attachment(r: &MessageRef, index: usize) -> Result<Option<String>, String> {
    let Some(body) = DATA.with(|d| d.borrow().bodies.get(&key(r)).cloned()) else { return Ok(None) };
    let Some(attachment) = body.attachments.get(index) else { return Ok(None) };
    let name = filename::sanitise_file_name(&attachment.file_name);
    let Some(path) = rfd::FileDialog::new().set_file_name(&name).save_file() else { return Ok(None) };
    let bytes = mailparser::attachment_bytes(&body.raw, attachment.part_index).ok_or("添付ファイルを取り出せませんでした")?;
    std::fs::write(&path, bytes).map_err(|e| format!("保存できませんでした: {e}"))?;
    mark_downloaded(&path);
    Ok(Some(path.display().to_string()))
}

#[cfg(windows)]
fn mark_downloaded(path: &std::path::Path) {
    // Mark-of-the-Web: Zone 3 (Internet) in the Zone.Identifier stream.
    let stream = format!("{}:Zone.Identifier", path.display());
    let _ = std::fs::write(stream, "[ZoneTransfer]\r\nZoneId=3\r\n");
}

#[cfg(target_os = "macos")]
fn mark_downloaded(path: &std::path::Path) {
    // Quarantine attribute, as browsers set it.
    let _ = std::process::Command::new("xattr")
        .args(["-w", "com.apple.quarantine", "0081;00000000;Fuu;"])
        .arg(path)
        .status();
}

#[cfg(not(any(windows, target_os = "macos")))]
fn mark_downloaded(_path: &std::path::Path) {}

/// Development aid (snapshots): use `raw` as the loaded body of `r`.
pub fn insert_body(r: &MessageRef, raw: Vec<u8>) {
    let body = Rc::new(process(Arc::new(raw)));
    DATA.with(|d| d.borrow_mut().bodies.insert(key(r), body));
}

/// The loaded body for a message tab, if any.
pub fn body_of(r: &MessageRef) -> Option<Rc<Body>> {
    DATA.with(|d| d.borrow().bodies.get(&key(r)).cloned())
}

/// Bytes of one attachment of a loaded message.
pub fn attachment_data(body: &Body, index: usize) -> Option<(String, String, Vec<u8>)> {
    let a = body.attachments.get(index)?;
    let data = mailparser::attachment_bytes(&body.raw, a.part_index)?;
    Some((filename::sanitise_file_name(&a.file_name), a.mime.clone(), data))
}

pub fn account_by_id(id: &str) -> Option<Account> {
    DATA.with(|d| d.borrow().accounts.iter().find(|a| a.id == id).cloned())
}

/// Hand a validated message to the engine.
pub fn send_outgoing(account_id: &str, outgoing: fuu_core::compose::Outgoing, answered: Option<(String, u32)>, draft_id: u64) {
    send(Command::Send { account_id: account_id.to_owned(), outgoing, answered, draft_id });
}

pub fn move_message(r: &MessageRef, target: MoveTarget) {
    if is_spike() {
        return;
    }
    send(Command::Move { account_id: r.account_id.clone(), folder: r.folder.clone(), uid: r.uid, target });
}

thread_local! {
    pub static DATA: RefCell<AppData> = RefCell::new(AppData::default());
}

pub fn is_spike() -> bool {
    DATA.with(|d| d.borrow().spike)
}

pub fn active_account() -> Option<Account> {
    DATA.with(|d| {
        let d = d.borrow();
        d.active.as_ref().and_then(|id| d.accounts.iter().find(|a| &a.id == id).cloned())
    })
}

fn send(command: Command) {
    let engine = DATA.with(|d| d.borrow().engine.clone());
    if let Some(engine) = engine {
        engine.send(command);
    }
}

/// Start the engine (or the spike data source) and load the accounts.
pub fn start(ui: &AppWindow, spike: bool) {
    if spike {
        DATA.with(|d| {
            let mut d = d.borrow_mut();
            d.spike = true;
            d.accounts = vec![Account {
                id: "spike".into(),
                display_name: "Fuu spike".into(),
                email: "spike@fuu.invalid".into(),
                username: String::new(),
                imap: ServerConfig { host: String::new(), port: 993, security: Security::Tls },
                smtp: ServerConfig { host: String::new(), port: 465, security: Security::Tls },
                auth: AuthKind::Password,
                oauth: None,
            }];
            d.active = Some("spike".into());
            d.folder = "INBOX".into();
        });
        render_accounts(ui);
        return;
    }
    let config_path = match fuu_core::config::default_path() {
        Ok(p) => p,
        Err(e) => {
            ui.set_list_message(e.to_string().into());
            return;
        }
    };
    let weak = ui.as_weak();
    let engine = Engine::start(config_path, fuu_core::cache::default_path(), Arc::new(KeychainStore), move |event| {
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                handle(&ui, event);
            }
        });
    });
    DATA.with(|d| d.borrow_mut().engine = Some(Arc::new(engine)));
    ui.set_list_busy(true);
    ui.set_list_failed(false);
    ui.set_list_message("読み込み中…".into());
    send(Command::LoadAccounts);
}

fn handle(ui: &AppWindow, event: Event) {
    match event {
        Event::Accounts(list) => {
            let previous = DATA.with(|d| d.borrow().active.clone());
            let active = previous.clone().filter(|id| list.iter().any(|a| &a.id == id)).or_else(|| list.first().map(|a| a.id.clone()));
            DATA.with(|d| {
                let mut d = d.borrow_mut();
                d.accounts = list;
                d.active = active.clone();
            });
            render_accounts(ui);
            match active {
                None => {
                    clear_mailbox(ui);
                    ui.set_list_busy(false);
                    ui.set_list_message("アカウントを追加すると、ここにメールが表示されます".into());
                }
                Some(id) if previous.as_ref() != Some(&id) => open_account(ui, &id),
                _ => {}
            }
        }
        Event::AccountAdded(account) => {
            ui.set_setup_oauth_waiting(false);
            let renewed = DATA.with(|d| {
                let mut d = d.borrow_mut();
                d.reauth_running = false;
                if d.reauth.as_deref() == Some(account.id.as_str()) {
                    d.reauth = None;
                    true
                } else {
                    false
                }
            });
            if renewed {
                ui.set_reauth_email(SharedString::default());
                ui.set_reauth_error(SharedString::default());
                ui.set_reauth_busy(false);
            }
            let switched = DATA.with(|d| d.borrow_mut().active.replace(account.id.clone())) != Some(account.id.clone());
            if switched {
                // The engine opens the new account next; the old list is not its.
                clear_mailbox(ui);
            }
            render_accounts(ui);
            ui.set_setup_busy(false);
            ui.set_setup_error(SharedString::default());
            ui.set_setup_password(SharedString::default());
            ui.invoke_setup_finished();
        }
        Event::ReauthRequired { account_id, message } => {
            let email = DATA.with(|d| d.borrow().accounts.iter().find(|a| a.id == account_id).map(|a| a.email.clone()));
            let Some(email) = email else { return };
            DATA.with(|d| d.borrow_mut().reauth = Some(account_id));
            ui.set_reauth_email(email.into());
            ui.set_reauth_error(message.into());
            ui.set_reauth_busy(false);
        }
        Event::OAuthBrowser { url } => {
            // Only provider sign-in URLs built by the engine arrive here.
            if url.starts_with("https://accounts.google.com/") || url.starts_with("https://login.microsoftonline.com/") {
                ui.set_setup_oauth_waiting(true);
                if let Err(e) = open::that_detached(&url) {
                    ui.set_setup_error(format!("ブラウザを開けませんでした: {e}").into());
                }
            }
        }
        Event::AccountAddFailed(message) if DATA.with(|d| d.borrow().reauth_running) => {
            DATA.with(|d| d.borrow_mut().reauth_running = false);
            ui.set_reauth_busy(false);
            ui.set_reauth_error(message.into());
        }
        Event::AccountAddFailed(message) => {
            ui.set_setup_oauth_waiting(false);
            ui.set_setup_busy(false);
            ui.set_setup_error(message.into());
        }
        Event::Folders { account_id, folders } => {
            if DATA.with(|d| d.borrow().active.as_deref() == Some(account_id.as_str())) {
                let items: Vec<FolderItem> = folders.iter().map(folder_item).collect();
                ui.set_folders(ModelRc::new(VecModel::from(items)));
                DATA.with(|d| d.borrow_mut().folders = folders);
            }
        }
        Event::FolderChanged { account_id, path } => {
            let shown = DATA.with(|d| {
                let d = d.borrow();
                d.active.as_deref() == Some(account_id.as_str()) && d.folder == path
            });
            if shown && !is_spike() {
                send(Command::RefreshFolder { account_id, path });
            }
        }
        Event::Messages { account_id, path, page, refresh } => {
            let (active, same_folder) = DATA.with(|d| {
                let d = d.borrow();
                (d.active.as_deref() == Some(account_id.as_str()), d.folder == path)
            });
            if !active || (refresh && !same_folder) {
                return;
            }
            if same_folder {
                // Same folder again (new mail, or reopened): edit the list in place
                // and keep the selected message selected.
                let selected = DATA.with(|d| {
                    let d = d.borrow();
                    usize::try_from(ui.get_current_row()).ok().and_then(|i| d.summaries.get(i)).map(|m| m.uid)
                });
                let rows = list_rows(&page.messages);
                let model = ui.get_messages();
                match model.as_any().downcast_ref::<VecModel<MessageRow>>() {
                    Some(vec) => crate::sync_rows(vec, &rows),
                    None => crate::set_message_model(&ui, rows),
                }
                let row = selected.and_then(|uid| page.messages.iter().position(|m| m.uid == uid));
                ui.set_current_row(row.map_or(-1, |i| i as i32));
                let empty = page.messages.is_empty();
                DATA.with(|d| d.borrow_mut().summaries = page.messages);
                ui.set_list_busy(false);
                ui.set_list_message(if empty { "メールはありません".into() } else { SharedString::default() });
                ui.set_list_failed(false);
                return;
            }
            let display = DATA.with(|d| d.borrow().folders.iter().find(|f| f.path == path).map(folder_title)).unwrap_or_else(|| path.clone());
            ui.set_mailbox(display.into());
            ui.set_current_folder(path.clone().into());
            let rows = list_rows(&page.messages);
            let empty = rows.is_empty();
            crate::set_message_model(&ui, rows);
            ui.set_current_row(-1);
            DATA.with(|d| {
                let mut d = d.borrow_mut();
                d.folder = path;
                d.summaries = page.messages;
            });
            ui.set_list_busy(false);
            ui.set_list_message(if empty { "メールはありません".into() } else { SharedString::default() });
                ui.set_list_failed(false);
        }
        Event::Body { account_id, folder, uid, raw } => {
            let k = (account_id, folder, uid);
            let body = Rc::new(process(raw));
            DATA.with(|d| {
                let mut d = d.borrow_mut();
                d.pending.retain(|p| p != &k);
                d.failures.remove(&k);
                d.bodies.insert(k, body);
            });
            ui.invoke_body_ready();
        }
        Event::BodyFailed { account_id, folder, uid, message } => {
            let k = (account_id, folder, uid);
            DATA.with(|d| {
                let mut d = d.borrow_mut();
                d.pending.retain(|p| p != &k);
                d.failures.insert(k, format!("本文を読み込めませんでした: {message}"));
            });
            ui.invoke_body_ready();
        }
        Event::RemoteImages { account_id, folder, uid, images } => {
            let k = (account_id, folder, uid);
            let fetched = images.iter().filter(|i| i.is_some()).count();
            DATA.with(|d| {
                let mut d = d.borrow_mut();
                d.remote_pending.retain(|p| p != &k);
                if let Some(body) = d.bodies.get(&k).cloned() {
                    let doc = fuu_sanitiser::with_remote_images(&body.doc, &images);
                    d.allowed.insert(k, Rc::new(doc));
                }
            });
            crate::notify(&ui, format!("外部画像を {fetched} / {} 件読み込みました", images.len()), false);
            ui.invoke_body_ready();
        }
        Event::Moved { account_id, folder, uid, target } => {
            let removed_row = DATA.with(|d| {
                let mut d = d.borrow_mut();
                d.bodies.remove(&(account_id.clone(), folder.clone(), uid));
                if d.active.as_deref() == Some(account_id.as_str()) && d.folder == folder {
                    let row = d.summaries.iter().position(|m| m.uid == uid);
                    if let Some(row) = row {
                        d.summaries.remove(row);
                    }
                    row
                } else {
                    None
                }
            });
            if let Some(row) = removed_row {
                if let Some(vec) = ui.get_messages().as_any().downcast_ref::<VecModel<MessageRow>>() {
                    let was_header = vec.row_data(row).is_some_and(|r| r.header);
                    vec.remove(row);
                    crate::sections::removed(vec, row, was_header);
                    crate::sections::renumber(vec, row);
                }
            }
            let what = if target == MoveTarget::Trash { "ゴミ箱に移動しました" } else { "アーカイブしました" };
            crate::notify(&ui, what, false);
            ui.invoke_message_moved(account_id.into(), folder.into(), uid as i32);
        }
        Event::Sent { draft_id, warning, .. } => {
            crate::notify(&ui, match warning {
                Some(w) => SharedString::from(format!("送信しました（{w}）")),
                None => SharedString::from("送信しました"),
            }, false);
            ui.invoke_draft_sent(draft_id as i32);
        }
        Event::SendFailed { draft_id, message } => ui.invoke_draft_failed(draft_id as i32, message.into()),
        Event::Status(text) => ui.set_feed_summary(text.into()),
        Event::Error(message) => {
            ui.set_list_busy(false);
            if ui.get_messages().row_count() == 0 {
                ui.set_list_message(message.clone().into());
                ui.set_list_failed(true);
            }
            crate::notify(&ui, message, true);
        }
    }
}

fn clear_mailbox(ui: &AppWindow) {
    crate::set_message_model(ui, Vec::new());
    ui.set_folders(ModelRc::new(VecModel::<FolderItem>::default()));
    ui.set_mailbox("受信トレイ".into());
    ui.set_current_folder(SharedString::default());
    DATA.with(|d| {
        let mut d = d.borrow_mut();
        d.folders.clear();
        d.summaries.clear();
        d.folder.clear();
    });
}

fn open_account(ui: &AppWindow, account_id: &str) {
    clear_mailbox(ui);
    ui.set_list_busy(true);
    ui.set_list_failed(false);
    ui.set_list_message("読み込み中…".into());
    send(Command::OpenAccount { account_id: account_id.to_owned(), folder: None });
}

pub fn account_item(a: &Account) -> AccountItem {
    AccountItem {
        id: a.id.clone().into(),
        name: if a.display_name.is_empty() { a.email.clone() } else { a.display_name.clone() }.into(),
        email: a.email.clone().into(),
        initial: a.initial().into(),
        hue: avatar_index(&a.email),
    }
}

pub fn render_accounts(ui: &AppWindow) {
    let (items, active_index, active) = DATA.with(|d| {
        let d = d.borrow();
        let items: Vec<AccountItem> = d.accounts.iter().map(account_item).collect();
        let index = d.active.as_ref().and_then(|id| d.accounts.iter().position(|a| &a.id == id));
        (items, index, index.map(|i| d.accounts[i].clone()))
    });
    ui.set_accounts(ModelRc::new(VecModel::from(items)));
    ui.set_active_account(active_index.map_or(-1, |i| i as i32));
    ui.set_active_oauth(active_is_oauth());
    match active {
        Some(a) => {
            let item = account_item(&a);
            ui.set_account_name(item.name);
            ui.set_account_email(item.email);
            ui.set_account_initial(item.initial);
            ui.set_account_hue(item.hue);
        }
        None => {
            ui.set_account_name("アカウントなし".into());
            ui.set_account_email("追加して始めましょう".into());
            ui.set_account_initial("+".into());
            ui.set_account_hue(0);
        }
    }
}

fn role_index(role: FolderRole) -> i32 {
    match role {
        FolderRole::Inbox => 0,
        FolderRole::Drafts => 1,
        FolderRole::Sent => 2,
        FolderRole::Archive => 3,
        FolderRole::Junk => 4,
        FolderRole::Trash => 5,
        FolderRole::Other => 6,
    }
}

/// Localised title for well-known folders, the decoded name otherwise.
fn folder_title(f: &Folder) -> String {
    match f.role {
        FolderRole::Inbox => "受信トレイ".into(),
        FolderRole::Drafts => "下書き".into(),
        FolderRole::Sent => "送信済み".into(),
        FolderRole::Archive => "アーカイブ".into(),
        FolderRole::Junk => "迷惑メール".into(),
        FolderRole::Trash => "ゴミ箱".into(),
        FolderRole::Other => f.display.clone(),
    }
}

fn folder_item(f: &Folder) -> FolderItem {
    FolderItem { path: f.path.clone().into(), display: folder_title(f).into(), role: role_index(f.role), selectable: f.selectable }
}

/// Today → time, this year → month/day, otherwise year/month/day.
pub fn format_date(timestamp: i64) -> String {
    let Some(date) = Local.timestamp_opt(timestamp, 0).single() else { return String::new() };
    let now = Local::now();
    if date.date_naive() == now.date_naive() {
        date.format("%H:%M").to_string()
    } else if date.year() == now.year() {
        format!("{}/{}", date.month(), date.day())
    } else {
        format!("{}/{}/{}", date.year(), date.month(), date.day())
    }
}

fn long_date(timestamp: i64) -> String {
    Local.timestamp_opt(timestamp, 0).single().map(|d| d.format("%Y/%m/%d %H:%M").to_string()).unwrap_or_default()
}

/// Bidirectional-control characters must never reach the UI (sender spoofing).
fn strip_bidi(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}')).collect()
}

/// List rows for a page of messages, with date sections marked.
fn list_rows(messages: &[MessageSummary]) -> Vec<MessageRow> {
    let today = crate::sections::today();
    let mut rows: Vec<MessageRow> = messages
        .iter()
        .enumerate()
        .map(|(i, m)| MessageRow {
            section: crate::sections::date_section(m.date, today).into(),
            index: i as i32,
            ..row_from_summary(m)
        })
        .collect();
    crate::sections::mark(&mut rows, None);
    rows
}

fn row_from_summary(m: &MessageSummary) -> MessageRow {
    let sender = strip_bidi(if m.from_name.is_empty() { &m.from_email } else { &m.from_name });
    MessageRow {
        initials: crate::initials(&sender).into(),
        avatar: avatar_index(&m.from_email),
        sender: sender.into(),
        subject: if m.subject.trim().is_empty() { "（件名なし）".into() } else { strip_bidi(&m.subject).into() },
        preview: strip_bidi(&m.from_email).into(),
        date: format_date(m.date).into(),
        unread: !m.seen,
        ..Default::default()
    }
}

/// Snapshot of list row `row` for a tab.
pub fn message_ref(ui: &AppWindow, row: usize) -> Option<MessageRef> {
    let (spike, account_id, folder, summary) = DATA.with(|d| {
        let d = d.borrow();
        (d.spike, d.active.clone().unwrap_or_default(), d.folder.clone(), d.summaries.get(row).cloned())
    });
    if spike {
        let r = ui.get_messages().row_data(row)?;
        return Some(MessageRef {
            account_id,
            folder,
            uid: row as u32,
            row,
            subject: r.subject.into(),
            from_name: r.sender.into(),
            from_email: format!("sender{row}@example.com"),
            // The list shows a time (today), month/day (this year) or a full date.
            date_text: match r.date.matches('/').count() {
                _ if r.date.contains(':') => format!("{} {}", Local::now().format("%Y/%m/%d"), r.date),
                1 => format!("{}/{} 10:{:02}", Local::now().year(), r.date, row % 60),
                _ => format!("{} 10:{:02}", r.date, row % 60),
            },
        });
    }
    let m = summary?;
    Some(MessageRef {
        account_id,
        folder,
        uid: m.uid,
        row,
        subject: strip_bidi(&m.subject),
        from_name: strip_bidi(&m.from_name),
        from_email: strip_bidi(&m.from_email),
        date_text: long_date(m.date),
    })
}

/// The list row currently showing this message, if it is in the visible folder.
pub fn row_of(r: &MessageRef) -> Option<usize> {
    DATA.with(|d| {
        let d = d.borrow();
        if d.spike {
            return Some(r.row);
        }
        if d.active.as_deref() != Some(r.account_id.as_str()) || d.folder != r.folder {
            return None;
        }
        d.summaries.iter().position(|m| m.uid == r.uid)
    })
}

// ───────────── Account setup form

fn security_index(s: Security) -> i32 {
    match s {
        Security::StartTls => 1,
        _ => 0,
    }
}

fn security_from(index: i32) -> Security {
    if index == 1 { Security::StartTls } else { Security::Tls }
}

/// Fill server settings from the address (presets, then domain guesses) unless the
/// user has edited them by hand.
pub fn setup_email_edited(ui: &AppWindow, email: &str) {
    let auto = DATA.with(|d| d.borrow().setup_auto) || ui.get_setup_imap_host().is_empty();
    if let Some(p) = account::preset_for(email) {
        ui.set_setup_provider(p.provider.into());
        ui.set_setup_note(p.note.unwrap_or_default().into());
        ui.set_setup_oauth_provider(p.oauth.map(|o| o.name()).unwrap_or_default().into());
        ui.set_setup_password_login(p.password_login);
        if auto {
            fill_servers(ui, &p.imap, &p.smtp);
        }
    } else {
        ui.set_setup_provider(SharedString::default());
        ui.set_setup_note(SharedString::default());
        ui.set_setup_oauth_provider(SharedString::default());
        ui.set_setup_password_login(true);
        if auto {
            if let Some((imap, smtp)) = account::guess_for(email) {
                fill_servers(ui, &imap, &smtp);
            }
        }
    }
    if auto {
        ui.set_setup_username(email.trim().into());
    }
}

fn fill_servers(ui: &AppWindow, imap: &ServerConfig, smtp: &ServerConfig) {
    ui.set_setup_imap_host(imap.host.clone().into());
    ui.set_setup_imap_port(imap.port.to_string().into());
    ui.set_setup_imap_security(security_index(imap.security));
    ui.set_setup_smtp_host(smtp.host.clone().into());
    ui.set_setup_smtp_port(smtp.port.to_string().into());
    ui.set_setup_smtp_security(security_index(smtp.security));
    DATA.with(|d| d.borrow_mut().setup_auto = true);
}

pub fn reset_setup(ui: &AppWindow) {
    for set in [
        AppWindow::set_setup_name,
        AppWindow::set_setup_email,
        AppWindow::set_setup_password,
        AppWindow::set_setup_username,
        AppWindow::set_setup_imap_host,
        AppWindow::set_setup_smtp_host,
        AppWindow::set_setup_provider,
        AppWindow::set_setup_note,
        AppWindow::set_setup_error,
    ] {
        set(ui, SharedString::default());
    }
    ui.set_setup_imap_port("993".into());
    ui.set_setup_smtp_port("465".into());
    ui.set_setup_imap_security(0);
    ui.set_setup_smtp_security(0);
    ui.set_setup_advanced(false);
    ui.set_setup_busy(false);
    ui.set_setup_oauth_provider(SharedString::default());
    ui.set_setup_password_login(true);
    ui.set_setup_oauth_waiting(false);
    DATA.with(|d| d.borrow_mut().setup_auto = true);
}

/// Validate the form and ask the engine to verify and add the account.
pub fn setup_submit(ui: &AppWindow) {
    let email = ui.get_setup_email().trim().to_owned();
    let password = ui.get_setup_password().to_string();
    let parse_port = |s: SharedString| s.trim().parse::<u16>().ok();
    let error = if !email.contains('@') {
        Some("メールアドレスを入力してください")
    } else if password.is_empty() {
        Some("パスワードを入力してください")
    } else if ui.get_setup_imap_host().trim().is_empty() || ui.get_setup_smtp_host().trim().is_empty() {
        ui.set_setup_advanced(true);
        Some("サーバーの詳細設定を入力してください")
    } else if parse_port(ui.get_setup_imap_port()).is_none() || parse_port(ui.get_setup_smtp_port()).is_none() {
        ui.set_setup_advanced(true);
        Some("ポート番号が正しくありません")
    } else {
        None
    };
    if let Some(e) = error {
        ui.set_setup_error(e.into());
        return;
    }
    if is_spike() {
        ui.set_setup_error("開発用データの表示中は追加できません（FUU_SPIKE_FEED を外して起動してください）".into());
        return;
    }
    let account = form_account(ui, email);
    ui.set_setup_error(SharedString::default());
    ui.set_setup_busy(true);
    send(Command::AddAccount { account, password });
}

/// Start a browser sign-in for the provider detected from the address.
pub fn setup_oauth_submit(ui: &AppWindow) {
    let email = ui.get_setup_email().trim().to_owned();
    let Some(provider) = account::preset_for(&email).and_then(|p| p.oauth) else {
        ui.set_setup_error("このアドレスはブラウザでのログインに対応していません".into());
        return;
    };
    if is_spike() {
        ui.set_setup_error("開発用データの表示中は追加できません（FUU_SPIKE_FEED を外して起動してください）".into());
        return;
    }
    let mut account = form_account(ui, email);
    account.auth = AuthKind::OAuth2;
    account.oauth = Some(provider);
    ui.set_setup_error(SharedString::default());
    ui.set_setup_busy(true);
    send(Command::AddOAuthAccount { account });
}

pub fn cancel_oauth() {
    send(Command::CancelOAuth);
}

/// Sign in again: the account needing it (banner), else the active account.
pub fn reauth(ui: &AppWindow) {
    let account = DATA.with(|d| {
        let d = d.borrow();
        let id = d.reauth.clone().or_else(|| d.active.clone())?;
        d.accounts.iter().find(|a| a.id == id).cloned()
    });
    let Some(account) = account.filter(|a| a.auth == AuthKind::OAuth2 && a.oauth.is_some()) else { return };
    if is_spike() {
        return;
    }
    DATA.with(|d| {
        let mut d = d.borrow_mut();
        d.reauth = Some(account.id.clone());
        d.reauth_running = true;
    });
    ui.set_reauth_email(account.email.clone().into());
    ui.set_reauth_error(SharedString::default());
    ui.set_reauth_busy(true);
    send(Command::AddOAuthAccount { account });
}

pub fn cancel_reauth(ui: &AppWindow) {
    send(Command::CancelOAuth);
    ui.set_reauth_busy(false);
}

/// Whether the active account signs in with OAuth (offers "sign in again").
pub fn active_is_oauth() -> bool {
    DATA.with(|d| {
        let d = d.borrow();
        d.active.as_ref().and_then(|id| d.accounts.iter().find(|a| &a.id == id)).is_some_and(|a| a.auth == AuthKind::OAuth2)
    })
}

fn form_account(ui: &AppWindow, email: String) -> Account {
    let parse_port = |s: SharedString| s.trim().parse::<u16>().ok();
    let username = ui.get_setup_username().trim().to_owned();
    Account {
        id: String::new(),
        display_name: ui.get_setup_name().trim().to_owned(),
        email: email.clone(),
        username: if username.is_empty() { email } else { username },
        imap: ServerConfig {
            host: ui.get_setup_imap_host().trim().to_owned(),
            port: parse_port(ui.get_setup_imap_port()).unwrap_or(993),
            security: security_from(ui.get_setup_imap_security()),
        },
        smtp: ServerConfig {
            host: ui.get_setup_smtp_host().trim().to_owned(),
            port: parse_port(ui.get_setup_smtp_port()).unwrap_or(465),
            security: security_from(ui.get_setup_smtp_security()),
        },
        auth: AuthKind::Password,
        oauth: None,
    }
}

pub fn select_account(ui: &AppWindow, index: usize) {
    let id = DATA.with(|d| d.borrow().accounts.get(index).map(|a| a.id.clone()));
    let Some(id) = id else { return };
    if is_spike() {
        return;
    }
    DATA.with(|d| d.borrow_mut().active = Some(id.clone()));
    render_accounts(ui);
    open_account(ui, &id);
}

pub fn select_folder(ui: &AppWindow, path: &str) {
    let Some(account) = active_account() else { return };
    if is_spike() {
        return;
    }
    ui.set_list_busy(true);
    ui.set_list_failed(false);
    send(Command::OpenFolder { account_id: account.id, path: path.to_owned() });
}

/// Load the shown folder again after a failure (or the account, when even the
/// folder list never arrived).
pub fn retry_load(ui: &AppWindow) {
    let Some(account) = active_account() else { return };
    let folder = DATA.with(|d| d.borrow().folder.clone());
    if folder.is_empty() {
        open_account(ui, &account.id);
    } else {
        select_folder(ui, &folder);
    }
}

pub fn remove_account(id: &str) {
    if !is_spike() {
        send(Command::RemoveAccount { account_id: id.to_owned() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_bidi_controls() {
        assert_eq!(strip_bidi("invoice\u{202E}fdp.exe"), "invoicefdp.exe");
        assert_eq!(strip_bidi("山田 太郎"), "山田 太郎");
    }

    #[test]
    fn dates_are_compact() {
        let now = Local::now();
        assert_eq!(format_date(now.timestamp()), now.format("%H:%M").to_string());
        let old = Local.with_ymd_and_hms(2001, 2, 3, 4, 5, 6).unwrap();
        assert_eq!(format_date(old.timestamp()), "2001/2/3");
    }
}
