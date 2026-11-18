//! Bridge between the mail engine (fuu-core) and the UI: accounts, folders, the
//! message list and the account-setup form. Everything here runs on the UI thread.

use std::cell::RefCell;
use std::sync::Arc;

use chrono::{Datelike, Local, TimeZone};
use fuu_core::account::{self, Account, AuthKind, Security, ServerConfig};
use fuu_core::engine::{Command, Engine, Event};
use fuu_core::imap::{Folder, FolderRole, MessageSummary};
use fuu_core::secrets::KeychainStore;
use fuu_model::{ManifestEntry, ResourceManifest, SafeDocument};
use fuu_sanitiser::escape_html;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::tabs::MessageRef;
use crate::{avatar_index, AccountItem, AppWindow, FolderItem, MessageRow};

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
            DATA.with(|d| d.borrow_mut().active = Some(account.id.clone()));
            render_accounts(ui);
            ui.set_setup_busy(false);
            ui.set_setup_error(SharedString::default());
            ui.set_setup_password(SharedString::default());
            ui.invoke_setup_finished();
        }
        Event::AccountAddFailed(message) => {
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
        Event::Messages { account_id, path, page } => {
            if DATA.with(|d| d.borrow().active.as_deref() != Some(account_id.as_str())) {
                return;
            }
            let display = DATA.with(|d| d.borrow().folders.iter().find(|f| f.path == path).map(folder_title)).unwrap_or_else(|| path.clone());
            ui.set_mailbox(display.into());
            ui.set_current_folder(path.clone().into());
            let rows: Vec<MessageRow> = page.messages.iter().map(row_from_summary).collect();
            let empty = rows.is_empty();
            ui.set_messages(ModelRc::new(VecModel::from(rows)));
            ui.set_current_row(-1);
            DATA.with(|d| {
                let mut d = d.borrow_mut();
                d.folder = path;
                d.summaries = page.messages;
            });
            ui.set_list_busy(false);
            ui.set_list_message(if empty { "メールはありません".into() } else { SharedString::default() });
        }
        Event::Status(text) => ui.set_feed_summary(text.into()),
        Event::Error(message) => {
            ui.set_list_busy(false);
            if ui.get_messages().row_count() == 0 {
                ui.set_list_message(message.clone().into());
            }
            ui.set_feed_summary(message.into());
        }
    }
}

fn clear_mailbox(ui: &AppWindow) {
    ui.set_messages(ModelRc::new(VecModel::<MessageRow>::default()));
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
            date_text: format!("2026/{} 10:{:02}", r.date, row % 60),
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

/// Body placeholder until M2 fetches and renders the real body.
pub fn placeholder_document(r: &MessageRef) -> SafeDocument {
    let text = format!(
        "本文の取得と表示は M2 で実装します。\n\n差出人: {} <{}>\n件名: {}\nUID: {}",
        r.from_name, r.from_email, r.subject, r.uid
    );
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><style>body{{font:14px/1.7 system-ui,'Segoe UI','Hiragino Sans','Yu Gothic UI',sans-serif;margin:6px 40px 48px;color:#8a877f}}</style></head><body><p>{}</p></body></html>",
        escape_html(&text).replace('\n', "<br>")
    );
    SafeDocument { html, text, resources: ResourceManifest { entries: Vec::<ManifestEntry>::new() }, flags: Default::default() }
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
        if auto {
            fill_servers(ui, &p.imap, &p.smtp);
        }
    } else {
        ui.set_setup_provider(SharedString::default());
        ui.set_setup_note(SharedString::default());
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
    let username = ui.get_setup_username().trim().to_owned();
    let account = Account {
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
    };
    ui.set_setup_error(SharedString::default());
    ui.set_setup_busy(true);
    send(Command::AddAccount { account, password });
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
    send(Command::OpenFolder { account_id: account.id, path: path.to_owned() });
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
