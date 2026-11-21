// Fuu — minimal desktop mailer: Slint UI, Blitz body renderer, IMAP engine
// (fuu-core) bridged through `mail`.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod editor;
mod mail;
mod os_mail;
mod preview;
mod samples;
mod sections;
mod tabs;
mod update;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use fuu_model::{Rect, RemoteContentPolicy, SafeDocument};
use fuu_render::blitz::{BlitzRenderer, RenderedPage};
use fuu_render::{BodyRenderer, LinkHandler, LinkRequest, PointerPhase, RenderStats};
use slint::{ComponentHandle, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};
use tabs::{AttachmentRef, Draft, DraftAttachment, MessageRef, Tab, TabContent, Tabs};
use fuu_i18n::tr;

slint::include_modules!();

const SPIKE_ROWS: usize = 5000;
/// Number of avatar colours in `Theme.avatars`.
const AVATAR_COLOURS: i32 = 8;
const BATCH: usize = 500;
/// Logical width reserved for the reader's vertical scrollbar.
const SCROLLBAR_ALLOWANCE: f32 = 16.0;
/// Re-layout delay for host-composited engines while the pane is being resized.
const RELAYOUT_DEBOUNCE: Duration = Duration::from_millis(120);

#[cfg(test)]
mod tests {
    use super::{avatar_index, initials};

    #[test]
    fn sync_rows_edits_in_place() {
        use slint::{Model, VecModel};
        let model = VecModel::from(vec![1, 2, 3, 4]);
        let check = |want: &[i32]| {
            super::sync_rows(&model, want);
            assert_eq!(model.iter().collect::<Vec<_>>(), want);
        };
        check(&[1, 2, 9, 3, 4]); // insert in the middle
        check(&[1, 9, 3, 4]); // remove
        check(&[4, 1, 9, 3]); // move
        check(&[4, 1, 7, 3]); // replace
        check(&[]);
        check(&[5]);
    }

    #[test]
    fn initials_for_latin_and_cjk_names() {
        assert_eq!(initials("Oliver Smith"), "OS");
        assert_eq!(initials("newsletter"), "N");
        assert_eq!(initials("佐藤 花子"), "佐");
        assert_eq!(initials("  "), "");
    }

    #[test]
    fn avatar_index_is_stable_and_in_range() {
        assert_eq!(avatar_index("a@example.com"), avatar_index("a@example.com"));
        assert!((0..super::AVATAR_COLOURS).contains(&avatar_index("x")));
    }
}

struct Reader {
    renderer: Box<dyn BodyRenderer>,
    stats: Option<Arc<RenderStats>>,
    last_rect: Rect,
    pending_rect: Option<(Rect, Instant)>,
    last_visible: bool,
    last_scale: f32,
    last_dark: bool,
}

#[derive(Default)]
struct ReaderState {
    reader: Option<Reader>,
    /// The document on screen.
    current: Option<SafeDocument>,
}

type SharedState = Rc<RefCell<ReaderState>>;

/// Initials for an avatar: the first character for CJK names, otherwise the first
/// letters of up to two words.
fn initials(name: &str) -> String {
    let name = name.trim();
    let Some(first) = name.chars().next() else { return String::new() };
    if !first.is_ascii() {
        return first.to_string();
    }
    name.split_whitespace()
        .take(2)
        .filter_map(|w| w.chars().next())
        .flat_map(char::to_uppercase)
        .collect()
}

/// Stable avatar colour index for a sender.
fn avatar_index(key: &str) -> i32 {
    let hash = key.bytes().fold(2166136261u32, |h, b| (h ^ u32::from(b)).wrapping_mul(16777619));
    (hash % AVATAR_COLOURS as u32) as i32
}

fn set_reader_header(ui: &AppWindow, subject: &str, name: &str, address: &str, date: &str) {
    ui.set_reader_subject(subject.into());
    ui.set_reader_name(name.into());
    ui.set_reader_address(address.into());
    ui.set_reader_initials(initials(name).into());
    ui.set_reader_avatar(avatar_index(address));
    ui.set_reader_date(date.into());
    ui.set_has_message(true);
}

fn show_document(state: &SharedState, doc: SafeDocument) {
    let mut st = state.borrow_mut();
    if let Some(r) = st.reader.as_mut() {
        if let Err(e) = r.renderer.show(&doc, RemoteContentPolicy::Block) {
            eprintln!("render failed: {e}");
        }
    }
    st.current = Some(doc);
}

/// Background tokio runtime standing in for the IMAP engine: rows arrive in
/// batches and are pushed into the Slint model on the UI thread.
fn spawn_mail_feed(ui: slint::Weak<AppWindow>) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_time()
            .build()
            .expect("tokio runtime");
        rt.block_on(async move {
            let started = Instant::now();
            let batches = SPIKE_ROWS / BATCH;
            // Spread the dummy mail over about two years, newest first.
            let now = chrono::Local::now().timestamp();
            let today = sections::today();
            for batch in 0..batches {
                tokio::time::sleep(Duration::from_millis(60)).await;
                let rows: Vec<(String, String, String, String, bool, String)> = (0..BATCH)
                    .map(|j| {
                        let n = batch * BATCH + j;
                        let date = now - n as i64 * 11_500;
                        (
                            ["佐藤 花子", "Oliver Smith", "鈴木一郎", "Newsletter", "山田 太郎"][n % 5].to_owned(),
                            format!("［#{n}］打ち合わせの件について — Re: colour proposal"),
                            "お疲れさまです。先日の件ですが、資料を添付しましたのでご確認ください。".to_owned(),
                            mail::format_date(date),
                            n % 7 == 0,
                            sections::date_section(date, today),
                        )
                    })
                    .collect();
                let ui = ui.clone();
                let last = batch + 1 == batches;
                let elapsed = started.elapsed();
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = ui.upgrade() else { return };
                    let model = ui.get_messages();
                    let Some(vec) = model.as_any().downcast_ref::<VecModel<MessageRow>>() else { return };
                    let offset = vec.row_count();
                    let mut rows: Vec<MessageRow> = rows
                        .into_iter()
                        .enumerate()
                        .map(|(k, (sender, subject, preview, date, unread, section))| MessageRow {
                            index: (offset + k) as i32,
                            initials: initials(&sender).into(),
                            avatar: avatar_index(&sender),
                            sender: sender.into(),
                            subject: subject.into(),
                            preview: preview.into(),
                            date: date.into(),
                            unread,
                            attachment: (offset + k) % 4 == 1,
                            section: section.into(),
                            ..Default::default()
                        })
                        .collect();
                    let before = vec.row_count().checked_sub(1).and_then(|i| vec.row_data(i)).map(|r| r.section);
                    sections::mark(&mut rows, before.as_deref());
                    vec.extend(rows);
                    if last {
                        ui.set_feed_summary(
                            format!("{} 件を {:.0} ms で取り込み", vec.row_count(), elapsed.as_secs_f64() * 1000.0).into(),
                        );
                    }
                });
            }
        });
    });
}

fn link_handler(ui: &AppWindow, pending_link: Rc<RefCell<Option<String>>>) -> LinkHandler {
    let link_ui = ui.as_weak();
    Box::new(move |link: LinkRequest| {
        let Some(ui) = link_ui.upgrade() else { return };
        match link {
            LinkRequest::Web(url) => {
                *pending_link.borrow_mut() = Some(url.clone());
                ui.set_link_url(url.into());
                ui.set_link_dialog_open(true);
            }
            // Same path as mailto: URLs from the system (see open_mailto).
            LinkRequest::Mailto(url) => os_mail::deliver(url),
        }
    })
}

fn new_reader(renderer: Box<dyn BodyRenderer>, stats: Option<Arc<RenderStats>>) -> Reader {
    Reader {
        renderer,
        stats,
        last_rect: Rect::default(),
        pending_rect: None,
        last_visible: true,
        last_scale: 0.0,
        last_dark: false,
    }
}

fn create_reader(ui: &AppWindow, pending_link: Rc<RefCell<Option<String>>>) -> Reader {
    ui.set_use_fallback(false);
    ui.set_use_page_image(true);
    let on_link = link_handler(ui, pending_link);
    let sink_ui = ui.as_weak();
    let sink = Box::new(move |page: RenderedPage| {
        let Some(ui) = sink_ui.upgrade() else { return };
        let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&page.rgba_premultiplied, page.width, page.height);
        ui.set_page_image(slint::Image::from_rgba8_premultiplied(buffer));
        ui.set_page_width(page.width as f32 / page.scale);
        ui.set_page_height(page.height as f32 / page.scale);
    });
    let renderer = BlitzRenderer::new(sink, on_link);
    let stats = renderer.stats();
    new_reader(Box::new(renderer), Some(stats))
}

type SharedTabs = Rc<RefCell<Tabs>>;

fn tab_limit_message() -> &'static str {
    tr!("You can have up to 32 tabs")
}

/// The signature block for a new draft from `account` (the active account
/// when None): that account's signature after a blank line, if it has one.
fn signature(account: Option<&str>) -> String {
    let account = match account {
        Some(id) => mail::account_by_id(id),
        None => mail::active_account(),
    };
    match account {
        Some(a) if a.signature_enabled && !a.signature.trim().is_empty() => format!("\n\n{}", a.signature),
        _ => String::new(),
    }
}

fn tab_title(tab: &Tab) -> String {
    match &tab.content {
        TabContent::Message(r) => if r.subject.trim().is_empty() { tr!("(no subject)").into() } else { r.subject.clone() },
        TabContent::Attachment(a) => a.name.clone(),
        TabContent::SecuritySample => "セキュリティ検査文書".into(),
        TabContent::CjkSample => "CJK 表示サンプル".into(),
        TabContent::Draft(d) => if d.subject.trim().is_empty() { d.title.clone() } else { d.subject.clone() },
        TabContent::Settings => tr!("Settings").into(),
        TabContent::AccountSetup => if mail::is_editing_account() { tr!("Edit account") } else { tr!("Add account") }.into(),
    }
}

fn tab_kind(tab: &Tab) -> i32 {
    match tab.content {
        TabContent::Draft(_) => 1,
        TabContent::Settings | TabContent::AccountSetup => 2,
        TabContent::Attachment(_) => 3,
        _ => 0,
    }
}

/// Delay between a close request and the actual removal (the shrink animation).
const TAB_CLOSE_ANIMATION: Duration = Duration::from_millis(170);

thread_local! {
    /// A tab that has already been closed but is still shrinking out in the tab
    /// strip: (model index, row, generation). The model then has one extra row, so
    /// indices coming from the UI are mapped with [`to_logical`].
    static GHOST: RefCell<Option<(usize, TabItem, u64)>> = const { RefCell::new(None) };
    static GHOST_GENERATION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn ghost_index() -> Option<usize> {
    GHOST.with(|g| g.borrow().as_ref().map(|(i, _, _)| *i))
}

/// Map a tab-strip (model) index to a logical tab index; `None` for the ghost.
fn to_logical(model_index: i32) -> Option<usize> {
    let m = usize::try_from(model_index).ok()?;
    match ghost_index() {
        Some(g) if m == g => None,
        Some(g) if m > g => Some(m - 1),
        _ => Some(m),
    }
}

/// Bring the tab model in line with `tabs` *in place*: only the rows that differ are
/// replaced, inserted or removed. Recreating the model would rebuild every tab
/// component, cancelling a drag in progress and the appear animation.
fn render_tabs(ui: &AppWindow, tabs: &Tabs) {
    let want: Vec<TabItem> = tabs
        .items()
        .iter()
        .map(|t| TabItem { title: tab_title(t).into(), kind: tab_kind(t), unseen: t.unseen, closing: false })
        .collect();
    let mut want = want;
    let ghost = GHOST.with(|g| g.borrow().clone());
    if let Some((g, row, _)) = &ghost {
        want.insert((*g).min(want.len()), row.clone());
    }
    let model = ui.get_tabs();
    match model.as_any().downcast_ref::<VecModel<TabItem>>() {
        Some(vec) => sync_rows(vec, &want),
        None => ui.set_tabs(ModelRc::new(VecModel::from(want))),
    }
    let to_model = |l: usize| match &ghost {
        Some((g, _, _)) if l >= *g => l + 1,
        _ => l,
    };
    ui.set_active_tab(tabs.active_index().map_or(-1, |i| to_model(i) as i32));
}

/// Install the message list: `messages` holds every row (indexed like the
/// summaries); `list-rows` is the view without rows of closed sections.
pub fn set_message_model(ui: &AppWindow, rows: Vec<MessageRow>) {
    let model = Rc::new(VecModel::from(rows));
    ui.set_messages(ModelRc::from(model.clone()));
    let visible = slint::FilterModel::new(model, |r: &MessageRow| r.header || !r.collapsed);
    ui.set_list_rows(ModelRc::new(visible));
}

/// Minimal edit from the current rows to `want`: keep the common prefix and suffix,
/// overwrite the overlapping middle, then insert or remove the remainder.
fn sync_rows<T: Clone + PartialEq + 'static>(model: &VecModel<T>, want: &[T]) {
    let old_len = model.row_count();
    let mut start = 0;
    while start < old_len && start < want.len() && model.row_data(start).as_ref() == Some(&want[start]) {
        start += 1;
    }
    let (mut end_old, mut end_new) = (old_len, want.len());
    while end_old > start && end_new > start && model.row_data(end_old - 1).as_ref() == Some(&want[end_new - 1]) {
        end_old -= 1;
        end_new -= 1;
    }
    let (old_mid, new_mid) = (end_old - start, end_new - start);
    let common = old_mid.min(new_mid);
    for k in 0..common {
        model.set_row_data(start + k, want[start + k].clone());
    }
    if old_mid > common {
        for _ in common..old_mid {
            model.remove(start + common);
        }
    } else {
        for k in common..new_mid {
            model.insert(start + k, want[start + k].clone());
        }
    }
}

/// Close a tab now (the neighbour activates and the pane switches immediately) and
/// let its row shrink out of the tab strip as a ghost.
fn request_close(ui: &AppWindow, state: &SharedState, tabs: &SharedTabs, index: usize) {
    if index >= tabs.borrow().items().len() {
        return;
    }
    save_draft(ui, tabs);
    // Settle a previous ghost first so indices line up.
    GHOST.with(|g| *g.borrow_mut() = None);
    render_tabs(ui, &tabs.borrow());
    let row = ui
        .get_tabs()
        .row_data(index)
        .map(|r| TabItem { closing: true, ..r });
    tabs.borrow_mut().close(index);
    let generation = GHOST_GENERATION.with(|c| {
        c.set(c.get() + 1);
        c.get()
    });
    if let Some(row) = row {
        GHOST.with(|g| *g.borrow_mut() = Some((index, row, generation)));
    }
    apply_active(ui, state, tabs);
    let (weak, tabs) = (ui.as_weak(), tabs.clone());
    slint::Timer::single_shot(TAB_CLOSE_ANIMATION, move || {
        let current = GHOST.with(|g| g.borrow().as_ref().map(|(_, _, gen)| *gen));
        if current != Some(generation) {
            return;
        }
        GHOST.with(|g| *g.borrow_mut() = None);
        if let Some(ui) = weak.upgrade() {
            render_tabs(&ui, &tabs.borrow());
        }
    });
}

/// Copy the composer fields back into the active draft tab.
fn save_draft(ui: &AppWindow, tabs: &SharedTabs) {
    if let Some(Tab { content: TabContent::Draft(d), .. }) = tabs.borrow_mut().active_mut() {
        d.from = ui.get_compose_from().into();
        d.to = ui.get_compose_to().into();
        d.cc = ui.get_compose_cc().into();
        d.bcc = ui.get_compose_bcc().into();
        d.subject = ui.get_compose_subject().into();
        d.doc = editor::current();
        d.show_cc_bcc = ui.get_compose_show_cc_bcc();
    }
}

/// Show the active tab in the right pane (or the empty state when there is none).
fn apply_active(ui: &AppWindow, state: &SharedState, tabs: &SharedTabs) {
    let content = tabs.borrow().active().map(|t| t.content.clone());
    ui.set_composing(false);
    ui.set_settings_open(false);
    ui.set_setup_open(false);
    ui.set_current_row(-1);
    ui.set_has_message(false);
    ui.set_viewing_attachment(false);
    ui.set_reader_to(SharedString::default());
    ui.set_reader_cc(SharedString::default());
    ui.set_attachments(ModelRc::new(VecModel::<AttachmentItem>::default()));
    ui.set_remote_blocked(false);
    ui.set_remote_allowable(false);
    ui.set_remote_loading(false);
    let doc = match content {
        None => None,
        Some(TabContent::Message(r)) => Some(show_message(ui, &r)),
        Some(TabContent::Attachment(a)) => {
            let subject = if a.message.subject.trim().is_empty() { tr!("(no subject)") } else { a.message.subject.as_str() };
            set_reader_header(ui, &a.name, &preview::describe(&a), "", &tr!("Attachment of “{0}”", subject));
            ui.set_viewing_attachment(true);
            Some(preview::document(ui, &a))
        }
        Some(TabContent::SecuritySample) => {
            set_reader_header(ui, "セキュリティ検査文書", "M0 spike", "spike@fuu.invalid", "");
            Some(samples::security_document())
        }
        Some(TabContent::CjkSample) => {
            set_reader_header(ui, "CJK：日本語・简体・繁體・한국어 ✉️", "髙﨑 𠮷子", "takasaki@example.jp", "");
            Some(samples::cjk_document())
        }
        Some(TabContent::Draft(d)) => {
            ui.set_compose_attachments(ModelRc::new(VecModel::from(draft_attachment_items(&d))));
            ui.set_compose_title(d.title.into());
            ui.set_compose_from(d.from.into());
            ui.set_compose_to(d.to.into());
            ui.set_compose_cc(d.cc.into());
            ui.set_compose_bcc(d.bcc.into());
            ui.set_compose_subject(d.subject.into());
            editor::load(ui, d.doc);
            ui.set_compose_show_cc_bcc(d.show_cc_bcc);
            ui.set_compose_error(d.error.into());
            ui.set_compose_sending(d.sending);
            ui.set_composing(true);
            None
        }
        Some(TabContent::Settings) => {
            ui.set_settings_open(true);
            None
        }
        Some(TabContent::AccountSetup) => {
            ui.set_setup_open(true);
            None
        }
    };
    if let Some(doc) = doc {
        ui.set_fallback_text(doc.text.clone().into());
        show_document(state, doc);
    }
    // Lets the pane play its reveal animation.
    ui.set_pane_version(ui.get_pane_version() + 1);
    render_tabs(ui, &tabs.borrow());
}

/// Header, list selection and body document for a message tab.
fn show_message(ui: &AppWindow, r: &MessageRef) -> SafeDocument {
    if let Some(row) = mail::row_of(r) {
        ui.set_current_row(row as i32);
        if let Some(data) = ui.get_messages().row_data(row) {
            if data.unread {
                if let Some(vec) = ui.get_messages().as_any().downcast_ref::<VecModel<MessageRow>>() {
                    vec.set_row_data(row, MessageRow { unread: false, ..data });
                }
            }
        }
    }
    let name = if r.from_name.is_empty() { r.from_email.as_str() } else { r.from_name.as_str() };
    let subject = if r.subject.trim().is_empty() { tr!("(no subject)") } else { r.subject.as_str() };
    set_reader_header(ui, subject, name, &r.from_email, &r.date_text);
    if mail::is_spike() {
        return samples::message_document(r.row, &r.subject);
    }
    match mail::body_or_request(r) {
        Ok(body) => {
            ui.set_reader_to(body.to.clone().into());
            ui.set_reader_cc(body.cc.clone().into());
            ui.set_attachments(ModelRc::new(VecModel::from(mail::attachment_items(&body))));
            let remote = mail::remote_state(r, &body);
            ui.set_remote_blocked(remote.blocked);
            ui.set_remote_allowable(remote.allowable);
            ui.set_remote_loading(remote.loading);
            remote.doc
        }
        Err(notice) => notice,
    }
}

/// Show a viewer (message or sample) in the active viewer tab, or a new tab.
fn show_viewer(ui: &AppWindow, state: &SharedState, tabs: &SharedTabs, content: TabContent) {
    save_draft(ui, tabs);
    if tabs.borrow_mut().show(content).is_err() {
        crate::notify(&ui, tab_limit_message(), false);
        return;
    }
    apply_active(ui, state, tabs);
}

thread_local! {
    static NEXT_DRAFT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

fn draft_attachment_items(d: &Draft) -> Vec<AttachmentItem> {
    d.attachments
        .iter()
        .map(|a| AttachmentItem {
            name: a.name.clone().into(),
            size: human_size(a.data.len() as u64).into(),
            executable: fuu_sanitiser::filename::is_executable(&a.name),
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

/// Total attachment size above which the composer warns (many servers reject ~25 MB).
const ATTACHMENT_LIMIT: usize = 20 * 1024 * 1024;

/// Open a draft in a new tab.
/// Settings: whether Fuu handles mailto: links.
fn refresh_default_mail_status(ui: &AppWindow) {
    let (text, is_default) = match os_mail::is_default() {
        Some(true) => (tr!("Fuu is your default mail app."), true),
        Some(false) => (tr!("Email links currently open another app."), false),
        None => (tr!("Available when Fuu runs as an app (Fuu.app)."), false),
    };
    ui.set_default_mail_status(text.into());
    ui.set_is_default_mail_app(is_default);
}

/// Bring the window forward (a mailto: request from the system).
fn raise_window(ui: &AppWindow) {
    use slint::winit_030::WinitWindowAccessor;
    ui.window().with_winit_window(|w| {
        w.set_minimized(false);
        w.focus_window();
    });
}

/// A new message from a `mailto:` URL (a link in a mail, or the system). The
/// URL is untrusted: fuu_core::mailto keeps only addresses, subject and body.
/// Waits (briefly) for the account list so the draft gets its sender.
fn open_mailto(ui: &AppWindow, state: &SharedState, tabs: &SharedTabs, url: String, attempt: u32) {
    if !mail::accounts_ready() && attempt < 50 {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        slint::Timer::single_shot(Duration::from_millis(200), move || {
            if let Some(ui) = weak.upgrade() {
                open_mailto(&ui, &state, &tabs, url, attempt + 1);
            }
        });
        return;
    }
    let Some(m) = fuu_core::mailto::parse(&url) else {
        notify(ui, tr!("Could not open the email link"), true);
        return;
    };
    let draft = Draft {
        title: tr!("New message").into(),
        to: m.to,
        cc: m.cc,
        bcc: m.bcc,
        subject: m.subject,
        doc: fuu_editor::model::RichDoc::from_plain(&format!("{}{}", m.body, signature(None))),
        ..Draft::default()
    };
    open_draft(ui, state, tabs, draft);
}

fn open_draft(ui: &AppWindow, state: &SharedState, tabs: &SharedTabs, mut draft: Draft) {
    save_draft(ui, tabs);
    let account = if draft.account_id.is_empty() { mail::active_account() } else { mail::account_by_id(&draft.account_id) };
    draft.from = match &account {
        Some(a) if a.display_name.is_empty() => a.email.clone(),
        Some(a) => format!("{} <{}>", a.display_name, a.email),
        None => tr!("(no account set up)").into(),
    };
    draft.account_id = account.map(|a| a.id).unwrap_or_default();
    draft.id = NEXT_DRAFT_ID.with(|c| {
        let id = c.get();
        c.set(id + 1);
        id
    });
    draft.show_cc_bcc = draft.show_cc_bcc || !draft.cc.is_empty() || !draft.bcc.is_empty();
    if tabs.borrow_mut().open(TabContent::Draft(draft), true).is_err() {
        crate::notify(&ui, tab_limit_message(), false);
        return;
    }
    apply_active(ui, state, tabs);
}

/// Close the active tab with its animation (send, discard, Ctrl/Cmd+W).
fn close_active(ui: &AppWindow, state: &SharedState, tabs: &SharedTabs) {
    let active = tabs.borrow().active_index();
    if let Some(i) = active {
        request_close(ui, state, tabs, i);
    }
}

/// Frameless chrome: macOS keeps its traffic lights over a transparent title bar;
/// Windows (and others) drop the native frame and Fuu draws its own controls.
fn select_backend() -> Result<(), slint::PlatformError> {
    slint::BackendSelector::new()
        .with_winit_window_attributes_hook(|attributes| {
            #[cfg(target_os = "macos")]
            {
                use slint::winit_030::winit::platform::macos::WindowAttributesExtMacOS;
                attributes.with_titlebar_transparent(true).with_title_hidden(true).with_fullsize_content_view(true)
            }
            #[cfg(target_os = "windows")]
            {
                use slint::winit_030::winit::platform::windows::WindowAttributesExtWindows;
                attributes.with_decorations(false).with_undecorated_shadow(true)
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            {
                use slint::winit_030::winit::platform::{wayland::WindowAttributesExtWayland, x11::WindowAttributesExtX11};
                // Wayland app id / X11 WM_CLASS: matches the desktop file
                // (fuu.desktop) so docks show Fuu's icon and name.
                let attributes = WindowAttributesExtWayland::with_name(attributes, "fuu", "fuu");
                WindowAttributesExtX11::with_name(attributes, "fuu", "fuu").with_decorations(false)
            }
        })
        .select()
}

/// The font Windows itself draws caption buttons with: Segoe Fluent Icons on
/// Windows 11, Segoe MDL2 Assets on Windows 10; "" when neither is installed.
fn caption_font() -> &'static str {
    #[cfg(windows)]
    {
        let fonts = std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts");
        if fonts.join("SegoeIcons.ttf").exists() {
            return "Segoe Fluent Icons";
        }
        if fonts.join("segmdl2.ttf").exists() {
            return "Segoe MDL2 Assets";
        }
    }
    ""
}

/// Platform touches that need the native window; false until it exists.
#[cfg(target_os = "macos")]
fn finish_native_window(ui: &AppWindow) -> bool {
    unify_mac_titlebar(ui)
}

/// Linux (X11): the window icon (Wayland takes it from the desktop file).
#[cfg(all(unix, not(target_os = "macos")))]
fn finish_native_window(ui: &AppWindow) -> bool {
    use slint::winit_030::winit::window::Icon;
    use slint::winit_030::WinitWindowAccessor;

    fn decode() -> Option<Icon> {
        let png = include_bytes!("../icons/app/fuu-128.png");
        let mut reader = png::Decoder::new(std::io::Cursor::new(&png[..])).read_info().ok()?;
        let mut rgba = vec![0; reader.output_buffer_size()?];
        let info = reader.next_frame(&mut rgba).ok()?;
        if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
            return None;
        }
        rgba.truncate(info.buffer_size());
        Icon::from_rgba(rgba, info.width, info.height).ok()
    }
    ui.window().with_winit_window(|w| w.set_window_icon(decode())).is_some()
}

/// Windows: the window and taskbar icons, loaded from the icon build.rs embeds
/// (resource 1) so Windows picks the right size from the .ico. Slint's `icon`
/// property would hand winit one large RGBA image, which the taskbar shrinks
/// crudely.
#[cfg(target_os = "windows")]
fn finish_native_window(ui: &AppWindow) -> bool {
    use slint::winit_030::winit::platform::windows::{IconExtWindows, WindowExtWindows};
    use slint::winit_030::winit::window::Icon;
    use slint::winit_030::WinitWindowAccessor;

    ui.window()
        .with_winit_window(|w| {
            match (Icon::from_resource(1, None), Icon::from_resource(1, None)) {
                (Ok(small), Ok(big)) => {
                    w.set_window_icon(Some(small));
                    w.set_taskbar_icon(Some(big));
                }
                (Err(e), _) | (_, Err(e)) => eprintln!("window icon: {e}"),
            }
        })
        .is_some()
}

/// macOS: give the window an empty toolbar in the unified style (as Mail and
/// Notes do). The title bar grows to the toolbar height and AppKit centres the
/// real traffic-light buttons in it, level with Fuu's own title-bar row.
#[cfg(target_os = "macos")]
fn unify_mac_titlebar(ui: &AppWindow) -> bool {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSToolbar, NSView, NSWindowToolbarStyle};
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use slint::winit_030::WinitWindowAccessor;

    let debug = std::env::var_os("FUU_DEBUG_CHROME").is_some();
    let Some(mtm) = MainThreadMarker::new() else {
        if debug { eprintln!("chrome: not on main thread") }
        return true;
    };
    let reached = ui.window().with_winit_window(|w| {
        let Ok(handle) = w.window_handle() else {
            if debug { eprintln!("chrome: no window handle") }
            return false;
        };
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else { return true };
        // SAFETY: winit hands out the window's live content view, and we are on
        // the main thread (checked above) inside the event loop.
        let view: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
        let Some(window) = view.window() else {
            if debug { eprintln!("chrome: view has no window") }
            return false;
        };
        let toolbar = NSToolbar::new(mtm);
        window.setToolbar(Some(&toolbar));
        window.setToolbarStyle(NSWindowToolbarStyle::Unified);
        if debug {
            use objc2_app_kit::NSWindowButton;
            let frame = window.frame();
            let layout = window.contentLayoutRect();
            eprintln!("chrome: window {frame:?} layout {layout:?} (title bar {:.1}pt)",
                frame.size.height - layout.size.height);
            if let Some(close) = window.standardWindowButton(NSWindowButton::CloseButton) {
                let f = close.frame();
                let sup = unsafe { close.superview() }.map(|s| s.frame());
                eprintln!("chrome: close button {f:?} in {sup:?}");
            }
        }
        true
    });
    if debug { eprintln!("chrome: winit window reached = {reached:?}") }
    reached.unwrap_or(false)
}

fn install_window_chrome(ui: &AppWindow) {
    use slint::winit_030::winit::window::ResizeDirection;
    use slint::winit_030::WinitWindowAccessor;

    ui.set_is_mac(cfg!(target_os = "macos"));
    ui.set_is_linux(cfg!(all(unix, not(target_os = "macos"))));
    ui.set_caption_font(caption_font().into());
    {
        let weak = ui.as_weak();
        ui.on_drag_window(move || {
            if let Some(ui) = weak.upgrade() {
                ui.window().with_winit_window(|w| {
                    let _ = w.drag_window();
                });
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_minimise_window(move || {
            if let Some(ui) = weak.upgrade() {
                ui.window().set_minimized(true);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_toggle_maximise(move || {
            if let Some(ui) = weak.upgrade() {
                let maximised = !ui.window().is_maximized();
                ui.window().set_maximized(maximised);
            }
        });
    }
    ui.on_close_window(|| {
        mail::flush_signatures_on_quit();
        let _ = slint::quit_event_loop();
    });
    {
        let weak = ui.as_weak();
        ui.on_resize_window(move |direction| {
            let Some(ui) = weak.upgrade() else { return };
            let direction = match direction {
                0 => ResizeDirection::North,
                1 => ResizeDirection::South,
                2 => ResizeDirection::East,
                3 => ResizeDirection::West,
                4 => ResizeDirection::NorthEast,
                5 => ResizeDirection::NorthWest,
                6 => ResizeDirection::SouthEast,
                _ => ResizeDirection::SouthWest,
            };
            ui.window().with_winit_window(|w| {
                let _ = w.drag_resize_window(direction);
            });
        });
    }
}

thread_local! {
    static NOTICE_SERIAL: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Show a short notice at the bottom of the window; it hides itself after a
/// few seconds (errors stay longer and can be closed).
pub(crate) fn notify(ui: &AppWindow, text: impl Into<SharedString>, error: bool) {
    let text: SharedString = text.into();
    let notice = ui.global::<Notice>();
    let serial = NOTICE_SERIAL.with(|s| {
        s.set(s.get() + 1);
        s.get()
    });
    if text.is_empty() {
        notice.set_shown(false);
        return;
    }
    notice.set_text(text);
    notice.set_error(error);
    notice.set_shown(true);
    let weak = ui.as_weak();
    let linger = if error { Duration::from_secs(10) } else { Duration::from_secs(4) };
    slint::Timer::single_shot(linger, move || {
        if NOTICE_SERIAL.with(|s| s.get()) == serial {
            if let Some(ui) = weak.upgrade() {
                ui.global::<Notice>().set_shown(false);
            }
        }
    });
}

/// Supply mixed-script fallbacks where the Windows system font is incomplete.
/// Preserve an explicit user-provided chain and register only installed fonts.
#[cfg(target_os = "windows")]
fn configure_font_fallbacks() {
    if std::env::var_os("SLINT_FONT_PATH").is_some() { return; }
    let fonts = std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts");
    let paths: Vec<_> = ["segoeui.ttf", "yugothm.ttc", "msyh.ttc", "msjh.ttc", "malgun.ttf", "seguiemj.ttf"]
        .iter().map(|name| fonts.join(name)).filter(|path| path.is_file()).collect();
    if !paths.is_empty() {
        if let Ok(value) = std::env::join_paths(paths) { std::env::set_var("SLINT_FONT_PATH", value); }
    }
}

#[cfg(target_os = "macos")]
fn configure_font_fallbacks() {
    if std::env::var_os("SLINT_FONT_PATH").is_some() { return; }
    let fonts = std::path::Path::new("/System/Library/Fonts");
    let paths: Vec<_> = ["SFNS.ttf", "ヒラギノ角ゴシック W3.ttc", "Hiragino Sans GB.ttc", "STHeiti Medium.ttc", "AppleSDGothicNeo.ttc", "Apple Color Emoji.ttc", "Supplemental/Arial Unicode.ttf"]
        .iter().map(|name| fonts.join(name)).filter(|path| path.is_file()).collect();
    if !paths.is_empty() {
        if let Ok(value) = std::env::join_paths(paths) { std::env::set_var("SLINT_FONT_PATH", value); }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn configure_font_fallbacks() {}

/// Switch the UI language (`""` follows the system): Slint's bundled
/// translations and fuu-i18n for strings built in Rust.
fn apply_language(ui: &AppWindow, choice: &str) {
    let code = if choice.is_empty() {
        fuu_i18n::language_for_locale(&sys_locale::get_locale().unwrap_or_default())
    } else {
        choice
    };
    let used = fuu_i18n::set_language(code);
    #[cfg(target_os = "windows")]
    ui.set_ui_font_family(match used {
        "zh-Hans" => "Microsoft YaHei UI",
        "zh-Hant" => "Microsoft JhengHei UI",
        "ja" => "Yu Gothic UI",
        "ko" => "Malgun Gothic",
        _ => "",
    }.into());
    #[cfg(target_os = "macos")]
    ui.set_ui_font_family(match used {
        "zh-Hans" => "Hiragino Sans GB",
        "zh-Hant" => "Heiti TC",
        "ja" => "Hiragino Sans",
        "ko" => "Apple SD Gothic Neo",
        _ => "",
    }.into());
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _ = ui;
    let _ = slint::select_bundled_translation(if used == "en" { "" } else { used });
}

/// Preferences (prefs.toml) and updates. Dummy-data and snapshot runs keep the
/// defaults, never save and never check for updates.
fn install_preferences_and_updates(ui: &AppWindow) {
    let dummy = std::env::var_os("FUU_SPIKE_FEED").is_some() || std::env::var_os("FUU_SNAPSHOT").is_some();
    let path = (!dummy).then(|| fuu_core::config::prefs_path().ok()).flatten();
    let prefs = path.as_deref().map(fuu_core::config::load_prefs).unwrap_or_default();
    ui.set_theme_mode(i32::from(prefs.theme.min(2)));
    ui.set_compose_html(prefs.compose_html);
    ui.set_check_updates(prefs.check_updates);
    ui.set_app_version(update::current_version().into());
    // Language: the saved choice, else the system's (FUU_LANG for dummy runs).
    let names: Vec<slint::SharedString> = std::iter::once(slint::SharedString::from(fuu_i18n::tr!("System default")))
        .chain(fuu_i18n::LANGUAGES.iter().map(|(_, name)| (*name).into()))
        .collect();
    ui.set_language_names(ModelRc::new(VecModel::from(names)));
    let chosen = if dummy { std::env::var("FUU_LANG").unwrap_or_default() } else { prefs.language.clone() };
    let index = fuu_i18n::LANGUAGES.iter().position(|(code, _)| *code == chosen).map_or(0, |i| i + 1);
    apply_language(ui, &chosen);
    ui.set_language_index(index as i32);

    let weak = ui.as_weak();
    ui.on_prefs_changed(move || {
        let Some(ui) = weak.upgrade() else { return };
        let language = usize::try_from(ui.get_language_index() - 1)
            .ok()
            .and_then(|i| fuu_i18n::LANGUAGES.get(i))
            .map_or(String::new(), |(code, _)| (*code).to_owned());
        apply_language(&ui, &language);
        let Some(path) = path.as_deref() else { return };
        let prefs = fuu_core::config::Preferences {
            check_updates: ui.get_check_updates(),
            theme: u8::try_from(ui.get_theme_mode().clamp(0, 2)).unwrap_or(0),
            compose_html: ui.get_compose_html(),
            language,
        };
        if let Err(e) = fuu_core::config::save_prefs(path, &prefs) {
            notify(&ui, tr!("Could not save settings: {0}", e), true);
        }
    });
    let weak = ui.as_weak();
    ui.on_update_install(move || {
        if let Some(ui) = weak.upgrade() {
            update::install(&ui);
        }
    });
    ui.on_update_restart(update::restart);
    let weak = ui.as_weak();
    ui.on_check_update_now(move || {
        if let Some(ui) = weak.upgrade() {
            update::check(&ui, true);
        }
    });
    if !dummy && prefs.check_updates {
        let weak = ui.as_weak();
        slint::Timer::single_shot(Duration::from_secs(5), move || {
            if let Some(ui) = weak.upgrade() {
                update::check(&ui, false);
            }
        });
    }
}

fn main() -> Result<(), slint::PlatformError> {
    // One Fuu per user: a second launch (a mailto: click on Windows, or
    // starting it again) hands its URL to the running Fuu and exits. Dummy-data
    // and snapshot runs stay independent of an installed Fuu.
    // (FUU_SINGLE_INSTANCE=1 turns the check on for such runs, for testing.)
    let dev_run = (std::env::var_os("FUU_SPIKE_FEED").is_some() || std::env::var_os("FUU_SNAPSHOT").is_some())
        && std::env::var_os("FUU_SINGLE_INSTANCE").is_none();
    let launch_url = os_mail::mailto_argument();
    if std::env::var_os("FUU_RELAUNCHED").is_some() {
        // Restarted after an update: let the old process release the instance lock.
        std::thread::sleep(Duration::from_millis(1200));
    }
    if !dev_run && !os_mail::claim_instance(launch_url.as_deref()) {
        return Ok(());
    }
    configure_font_fallbacks();
    select_backend()?;
    let ui = AppWindow::new()?;
    install_window_chrome(&ui);
    set_message_model(&ui, Vec::new());
    ui.set_feed_summary(tr!("Loading…").into());
    install_preferences_and_updates(&ui);

    let state: SharedState = Rc::new(RefCell::new(ReaderState::default()));
    let pending_link: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    state.borrow_mut().reader = Some(create_reader(&ui, pending_link.clone()));
    let tabs: SharedTabs = Rc::new(RefCell::new(Tabs::default()));
    ui.set_tabs(ModelRc::new(VecModel::<TabItem>::default()));
    render_tabs(&ui, &tabs.borrow());

    {
        let weak = ui.as_weak();
        ui.on_toggle_section(move |section| {
            let Some(ui) = weak.upgrade() else { return };
            let model = ui.get_messages();
            let Some(vec) = model.as_any().downcast_ref::<VecModel<MessageRow>>() else { return };
            sections::toggle(vec, &section);
        });
        let weak = ui.as_weak();
        ui.on_toggle_conversation(move |row| {
            if let (Some(ui), Ok(row)) = (weak.upgrade(), usize::try_from(row)) {
                mail::toggle_conversation(&ui, row);
            }
        });
        let weak = ui.as_weak();
        ui.on_step_row(move |from, dir| {
            weak.upgrade().map_or(from, |ui| sections::step(&ui.get_messages(), from, dir))
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_conversation_selected(move |row, child| {
            let Some(ui) = weak.upgrade() else { return };
            let (Ok(row), Ok(child)) = (usize::try_from(row), usize::try_from(child)) else { return };
            if let Some(r) = mail::conversation_ref(&ui, row, child) {
                show_viewer(&ui, &state, &tabs, TabContent::Message(r));
            }
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_row_selected(move |i| {
            let Some(ui) = weak.upgrade() else { return };
            if let Some(r) = mail::message_ref(&ui, i as usize) {
                show_viewer(&ui, &state, &tabs, TabContent::Message(r));
            }
        });
    }
    {
        let (tabs, weak) = (tabs.clone(), ui.as_weak());
        ui.on_row_open_in_tab(move |i| {
            let Some(ui) = weak.upgrade() else { return };
            save_draft(&ui, &tabs);
            let Some(r) = mail::message_ref(&ui, i as usize) else { return };
            if tabs.borrow_mut().open(TabContent::Message(r), false).is_err() {
                crate::notify(&ui, tab_limit_message(), false);
            }
            render_tabs(&ui, &tabs.borrow());
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_load_security_sample(move || {
            if let Some(ui) = weak.upgrade() {
                show_viewer(&ui, &state, &tabs, TabContent::SecuritySample);
            }
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_load_cjk_sample(move || {
            if let Some(ui) = weak.upgrade() {
                show_viewer(&ui, &state, &tabs, TabContent::CjkSample);
            }
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_open_settings(move || {
            let Some(ui) = weak.upgrade() else { return };
            save_draft(&ui, &tabs);
            if tabs.borrow_mut().show_single(TabContent::Settings).is_err() {
                crate::notify(&ui, tab_limit_message(), false);
                return;
            }
            apply_active(&ui, &state, &tabs);
        });
    }
    // Accounts and folders
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_open_account_setup(move || {
            let Some(ui) = weak.upgrade() else { return };
            save_draft(&ui, &tabs);
            let already_open = tabs.borrow().items().iter().any(|t| t.content == TabContent::AccountSetup);
            if !already_open {
                mail::reset_setup(&ui);
            }
            if tabs.borrow_mut().show_single(TabContent::AccountSetup).is_err() {
                crate::notify(&ui, tab_limit_message(), false);
                return;
            }
            apply_active(&ui, &state, &tabs);
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_setup_finished(move || {
            let Some(ui) = weak.upgrade() else { return };
            tabs.borrow_mut().close_content(&TabContent::AccountSetup);
            apply_active(&ui, &state, &tabs);
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_setup_email_edited(move |email| {
            if let Some(ui) = weak.upgrade() {
                mail::setup_email_edited(&ui, &email);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_setup_submit(move || {
            if let Some(ui) = weak.upgrade() {
                mail::setup_submit(&ui);
            }
        });
        let weak = ui.as_weak();
        ui.on_setup_oauth_submit(move || {
            if let Some(ui) = weak.upgrade() {
                mail::setup_oauth_submit(&ui);
            }
        });
        ui.on_setup_oauth_cancel(mail::cancel_oauth);
        let weak = ui.as_weak();
        ui.on_retry_load(move || {
            if let Some(ui) = weak.upgrade() {
                mail::retry_load(&ui);
            }
        });
        let weak = ui.as_weak();
        ui.on_reauth(move || {
            if let Some(ui) = weak.upgrade() {
                mail::reauth(&ui);
            }
        });
        let weak = ui.as_weak();
        ui.on_reauth_cancel(move || {
            if let Some(ui) = weak.upgrade() {
                mail::cancel_reauth(&ui);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_select_account(move |i| {
            if let Some(ui) = weak.upgrade() {
                mail::select_account(&ui, i as usize);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_select_folder(move |path| {
            if let Some(ui) = weak.upgrade() {
                mail::select_folder(&ui, &path);
            }
        });
    }
    ui.on_remove_account(move |id| mail::remove_account(&id));
    {
        // Settings → アカウント → 編集: the setup form, filled in.
        let (tabs, weak) = (tabs.clone(), ui.as_weak());
        ui.on_edit_account(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            ui.invoke_open_account_setup();
            mail::edit_account(&ui, &id);
            render_tabs(&ui, &tabs.borrow());
        });
    }
    {
        // A body arrived: refresh the pane if it belongs to the active tab.
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_body_ready(move || {
            let Some(ui) = weak.upgrade() else { return };
            let active_is_message = matches!(tabs.borrow().active().map(|t| &t.content), Some(TabContent::Message(_) | TabContent::Attachment(_)));
            if active_is_message {
                apply_active(&ui, &state, &tabs);
            }
        });
    }
    {
        // A finished preview redraws only an attachment tab (not a message the
        // user went back to meanwhile).
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_preview_ready(move || {
            let Some(ui) = weak.upgrade() else { return };
            if matches!(tabs.borrow().active().map(|t| &t.content), Some(TabContent::Attachment(_))) {
                apply_active(&ui, &state, &tabs);
            }
        });
    }
    {
        // "Show images" for the active message only.
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_allow_remote(move || {
            let Some(ui) = weak.upgrade() else { return };
            if let Some(TabContent::Message(r)) = tabs.borrow().active().map(|t| t.content.clone()) {
                mail::allow_remote(&r);
                ui.set_remote_loading(true);
            }
            let _ = &state;
        });
    }
    {
        // A moved message closes its tabs.
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_message_moved(move |account, folder, uid| {
            let Some(ui) = weak.upgrade() else { return };
            let matches = |t: &Tab| matches!(&t.content, TabContent::Message(r) if r.account_id == account.as_str() && r.folder == folder.as_str() && r.uid == uid as u32);
            loop {
                let index = tabs.borrow().items().iter().position(matches);
                match index {
                    Some(i) => tabs.borrow_mut().close(i),
                    None => break,
                }
            }
            apply_active(&ui, &state, &tabs);
        });
    }
    {
        // Attachments: executables need confirmation first.
        let (tabs, weak) = (tabs.clone(), ui.as_weak());
        let pending_exec = Rc::new(std::cell::Cell::new(None::<usize>));
        ui.on_save_attachment(move |index, executable| {
            let Some(ui) = weak.upgrade() else { return };
            let index = if index < 0 { match pending_exec.take() { Some(i) => i, None => return } } else { index as usize };
            if executable {
                pending_exec.set(Some(index));
                let name = ui.get_attachments().row_data(index).map(|a| a.name).unwrap_or_default();
                ui.set_exec_warning(name);
                return;
            }
            let (r, index) = match tabs.borrow().active().map(|t| &t.content) {
                Some(TabContent::Message(r)) => (r.clone(), index),
                // From an attachment tab: its own file.
                Some(TabContent::Attachment(a)) => (a.message.clone(), a.index),
                _ => return,
            };
            match mail::save_attachment(&r, index) {
                Ok(Some(path)) => crate::notify(&ui, tr!("Saved: {0}", path), false),
                Ok(None) => {}
                Err(e) => crate::notify(&ui, e, true),
            }
        });
    }
    {
        // Clicking an attachment previews it in its own tab when Fuu can show
        // it; anything else is saved (executables after the warning).
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_open_attachment(move |index, executable| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(TabContent::Message(r)) = tabs.borrow().active().map(|t| t.content.clone()) else { return };
            let attachment = mail::body_of(&r).and_then(|body| body.attachments.get(index as usize).cloned());
            let Some(attachment) = attachment else { return };
            let name = fuu_sanitiser::filename::sanitise_file_name(&attachment.file_name);
            if executable || !preview::previewable(&name, &attachment.mime) {
                ui.invoke_save_attachment(index, executable);
                return;
            }
            let size = ui.get_attachments().row_data(index as usize).map(|a| a.size.to_string()).unwrap_or_default();
            let content = TabContent::Attachment(AttachmentRef { message: r, index: index as usize, name, mime: attachment.mime.clone(), size });
            save_draft(&ui, &tabs);
            if tabs.borrow_mut().show_single(content).is_err() {
                crate::notify(&ui, tab_limit_message(), false);
                return;
            }
            apply_active(&ui, &state, &tabs);
        });
    }
    // Tabs
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_tab_activated(move |i| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(i) = to_logical(i) else { return };
            save_draft(&ui, &tabs);
            tabs.borrow_mut().activate(i);
            apply_active(&ui, &state, &tabs);
        });
    }
    {
        let (tabs, weak) = (tabs.clone(), ui.as_weak());
        ui.on_tab_moved(move |from, to| {
            let Some(ui) = weak.upgrade() else { return };
            let (Some(from), Some(to)) = (to_logical(from), to_logical(to)) else { return };
            tabs.borrow_mut().move_tab(from, to);
            render_tabs(&ui, &tabs.borrow());
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_tab_closed(move |i| {
            if let Some(ui) = weak.upgrade() {
                if let Some(i) = to_logical(i) {
                    request_close(&ui, &state, &tabs, i);
                }
            }
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_close_active_tab(move || {
            if let Some(ui) = weak.upgrade() {
                close_active(&ui, &state, &tabs);
            }
        });
    }
    {
        // Ctrl/Cmd+T and Ctrl/Cmd+N both start a new message.
        let weak = ui.as_weak();
        ui.on_new_tab(move || {
            if let Some(ui) = weak.upgrade() {
                ui.invoke_start_compose();
            }
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_reopen_closed_tab(move || {
            let Some(ui) = weak.upgrade() else { return };
            save_draft(&ui, &tabs);
            if tabs.borrow_mut().reopen().is_some() {
                apply_active(&ui, &state, &tabs);
            }
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_cycle_tab(move |delta| {
            let Some(ui) = weak.upgrade() else { return };
            save_draft(&ui, &tabs);
            tabs.borrow_mut().cycle(delta);
            apply_active(&ui, &state, &tabs);
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_select_tab(move |position| {
            let Some(ui) = weak.upgrade() else { return };
            save_draft(&ui, &tabs);
            tabs.borrow_mut().select(position);
            apply_active(&ui, &state, &tabs);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_page_clicked(move |x, y| {
            let Some(ui) = weak.upgrade() else { return };
            let scale = ui.window().scale_factor();
            // Take the renderer out of the borrow so link callbacks may touch state.
            let reader = state.borrow_mut().reader.take();
            if let Some(mut r) = reader {
                // A click that ends a text selection is not a link activation.
                if r.renderer.selected_text().is_none() {
                    r.renderer.pointer_click(x * scale, y * scale);
                }
                state.borrow_mut().reader = Some(r);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_page_pointer(move |phase, x, y| {
            let Some(ui) = weak.upgrade() else { return };
            let scale = ui.window().scale_factor();
            let phase = if phase == 0 { PointerPhase::Down } else { PointerPhase::Up };
            if let Some(r) = state.borrow_mut().reader.as_mut() {
                r.renderer.pointer_event(phase, x * scale, y * scale, phase == PointerPhase::Down);
            }
        });
    }
    {
        let state = state.clone();
        ui.on_select_all(move || {
            if let Some(r) = state.borrow_mut().reader.as_mut() {
                r.renderer.select_all();
            }
        });
    }
    {
        let state = state.clone();
        ui.on_clear_selection(move || {
            if let Some(r) = state.borrow_mut().reader.as_mut() {
                r.renderer.clear_selection();
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_copy_selection(move || {
            let Some(ui) = weak.upgrade() else { return };
            let text = state.borrow().reader.as_ref().and_then(|r| r.renderer.selected_text());
            let Some(text) = text else { return };
            match arboard::Clipboard::new().and_then(|mut c| c.set_text(text.clone())) {
                Ok(()) => crate::notify(&ui, tr!("Copied {0} characters", text.chars().count()), false),
                Err(e) => eprintln!("clipboard: {e}"),
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_page_pointer_moved(move |x, y, pressed| {
            let Some(ui) = weak.upgrade() else { return };
            let scale = ui.window().scale_factor();
            let mut st = state.borrow_mut();
            let Some(r) = st.reader.as_mut() else { return };
            if pressed {
                // Dragging a selection keeps the text cursor.
                r.renderer.pointer_event(PointerPhase::Move, x * scale, y * scale, true);
            } else {
                let cursor = r.renderer.hover(x * scale, y * scale).index();
                if cursor != ui.get_page_cursor() {
                    ui.set_page_cursor(cursor);
                }
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_page_pointer_left(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_page_cursor(0);
            }
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_start_compose(move || {
            if let Some(ui) = weak.upgrade() {
                let draft = Draft { title: tr!("New message").into(), doc: fuu_editor::model::RichDoc::from_plain(&signature(None)), ..Draft::default() };
                open_draft(&ui, &state, &tabs, draft);
            }
        });
    }
    {
        // mailto: requests: links in mails, the system (Windows: a second
        // launch; macOS: Apple Events), or "" to just show the window.
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        os_mail::set_sink(move |request| {
            let Some(ui) = weak.upgrade() else { return };
            raise_window(&ui);
            if !request.is_empty() {
                open_mailto(&ui, &state, &tabs, request, 0);
            }
        });
        #[cfg(target_os = "macos")]
        if let Some(mtm) = objc2::MainThreadMarker::new() {
            os_mail::install_url_handler(mtm);
        }
        if let Some(url) = launch_url.clone() {
            os_mail::deliver(url);
        }
        let weak = ui.as_weak();
        ui.on_make_default_mail_app(move || {
            let Some(ui) = weak.upgrade() else { return };
            match os_mail::make_default() {
                Ok(()) => {
                    #[cfg(windows)]
                    notify(&ui, tr!("Choose Fuu in Default apps to finish"), false);
                }
                Err(e) => notify(&ui, e, true),
            }
            refresh_default_mail_status(&ui);
        });
        let weak = ui.as_weak();
        ui.on_refresh_default_mail(move || {
            if let Some(ui) = weak.upgrade() {
                refresh_default_mail_status(&ui);
                mail::load_signature_editor(&ui, None);
            }
        });
        ui.on_settings_closed(mail::flush_signatures);
        let weak = ui.as_weak();
        ui.on_signature_edited(move || {
            if let Some(ui) = weak.upgrade() {
                mail::signature_edited(&ui);
            }
        });
        let weak = ui.as_weak();
        ui.on_signature_account_picked(move || {
            if let Some(ui) = weak.upgrade() {
                let index = usize::try_from(ui.get_signature_account()).ok();
                mail::load_signature_editor(&ui, index);
            }
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_compose_to_address(move |address| {
            if let Some(ui) = weak.upgrade() {
                let draft = Draft { title: tr!("New message").into(), to: address.into(), doc: fuu_editor::model::RichDoc::from_plain(&signature(None)), ..Draft::default() };
                open_draft(&ui, &state, &tabs, draft);
            }
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_reply(move |all| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(TabContent::Message(r)) = tabs.borrow().active().map(|t| t.content.clone()) else { return };
            let own: Vec<String> = mail::account_by_id(&r.account_id).map(|a| vec![a.email]).unwrap_or_default();
            let mut draft = Draft {
                title: if all { tr!("Reply all") } else { tr!("Reply") }.into(),
                subject: fuu_core::compose::reply_subject(&r.subject),
                account_id: r.account_id.clone(),
                answered: (!mail::is_spike()).then(|| (r.folder.clone(), r.uid)),
                ..Draft::default()
            };
            match mail::body_of(&r) {
                Some(body) => {
                    let (to, cc) = fuu_core::compose::reply_recipients(&body.headers, &own, all);
                    draft.to = to.iter().map(|x| x.display()).collect::<Vec<_>>().join(", ");
                    draft.cc = cc.iter().map(|x| x.display()).collect::<Vec<_>>().join(", ");
                    draft.in_reply_to = body.headers.message_id.clone();
                    draft.references = body.headers.references.clone();
                    if let Some(id) = &body.headers.message_id {
                        if !draft.references.contains(id) {
                            draft.references.push(id.clone());
                        }
                    }
                    draft.doc = editor::original_doc(&signature(Some(&r.account_id)), &fuu_core::compose::original_header(&body.headers), &body.doc.text);
                }
                None => {
                    draft.to = r.from_email.clone();
                    // Body not loaded: build the header block from the list entry.
                    let headers = fuu_model::Headers {
                        from: vec![fuu_model::Address {
                            name: (!r.from_name.is_empty()).then(|| r.from_name.clone()),
                            email: r.from_email.clone(),
                        }],
                        subject: r.subject.clone(),
                        ..Default::default()
                    };
                    draft.doc = editor::original_doc(&signature(Some(&r.account_id)), &fuu_core::compose::original_header(&headers), "");
                }
            }
            open_draft(&ui, &state, &tabs, draft);
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_forward(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(TabContent::Message(r)) = tabs.borrow().active().map(|t| t.content.clone()) else { return };
            let mut draft = Draft {
                title: tr!("Forward").into(),
                subject: fuu_core::compose::forward_subject(&r.subject),
                account_id: r.account_id.clone(),
                doc: fuu_editor::model::RichDoc::from_plain(&format!("\n{}", signature(Some(&r.account_id)))),
                ..Draft::default()
            };
            if let Some(body) = mail::body_of(&r) {
                draft.doc = editor::original_doc(&signature(Some(&r.account_id)), &fuu_core::compose::original_header(&body.headers), &body.doc.text);
                // Forward the original attachments too.
                draft.attachments = (0..body.attachments.len())
                    .filter_map(|i| mail::attachment_data(&body, i))
                    .map(|(name, mime, data)| DraftAttachment { name, mime, data })
                    .collect();
            }
            open_draft(&ui, &state, &tabs, draft);
        });
    }
    {
        let weak = ui.as_weak();
        let tabs = tabs.clone();
        ui.on_archive(move || {
            let Some(_ui) = weak.upgrade() else { return };
            if let Some(TabContent::Message(r)) = tabs.borrow().active().map(|t| t.content.clone()) {
                mail::move_message(&r, fuu_core::engine::MoveTarget::Archive);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let tabs = tabs.clone();
        ui.on_restore(move || {
            let Some(_ui) = weak.upgrade() else { return };
            if let Some(TabContent::Message(r)) = tabs.borrow().active().map(|t| t.content.clone()) {
                mail::move_message(&r, fuu_core::engine::MoveTarget::Inbox);
            }
        });
    }
    {
        // Hover actions on list rows act on that row, not the open message.
        let weak = ui.as_weak();
        ui.on_row_action(move |row, action| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(r) = usize::try_from(row).ok().and_then(|i| mail::message_ref(&ui, i)) else { return };
            use fuu_core::engine::MoveTarget;
            let target = match action {
                0 => MoveTarget::Trash,
                1 => MoveTarget::Archive,
                _ => MoveTarget::Inbox,
            };
            mail::move_message(&r, target);
        });
    }
    {
        let weak = ui.as_weak();
        let tabs = tabs.clone();
        ui.on_delete(move || {
            let Some(_ui) = weak.upgrade() else { return };
            if let Some(TabContent::Message(r)) = tabs.borrow().active().map(|t| t.content.clone()) {
                mail::move_message(&r, fuu_core::engine::MoveTarget::Trash);
            }
        });
    }
    {
        // Rich editor bridge (see editor.rs).
        let weak = ui.as_weak();
        ui.on_editor_edited(move |text| {
            if let Some(ui) = weak.upgrade() {
                editor::edited(&ui, &text);
            }
        });
        let weak = ui.as_weak();
        ui.on_editor_command(move |name| {
            if let Some(ui) = weak.upgrade() {
                editor::command(&ui, &name);
            }
        });
        let weak = ui.as_weak();
        ui.on_editor_navigate(move |key, extend| {
            if let Some(ui) = weak.upgrade() {
                editor::navigate(&ui, &key, extend, 400.0);
            }
        });
        let weak = ui.as_weak();
        ui.on_editor_enter(move || weak.upgrade().is_some_and(|ui| editor::enter(&ui)));
        let weak = ui.as_weak();
        ui.on_editor_pointer(move |kind, x, y, clicks, shift| {
            if let Some(ui) = weak.upgrade() {
                editor::pointer(&ui, kind, x, y, clicks, shift);
            }
        });
        let weak = ui.as_weak();
        ui.on_editor_caret_moved(move |anchor, cursor| {
            if let Some(ui) = weak.upgrade() {
                editor::caret_moved(&ui, anchor, cursor);
            }
        });
        let weak = ui.as_weak();
        ui.on_editor_preedit_changed(move |text| {
            if let Some(ui) = weak.upgrade() {
                editor::preedit_changed(&ui, &text);
            }
        });
        let weak = ui.as_weak();
        ui.on_editor_focus_gained(move || {
            if let Some(ui) = weak.upgrade() {
                editor::focus_gained(&ui);
            }
        });
        let weak = ui.as_weak();
        ui.on_editor_resized(move |width| {
            if let Some(ui) = weak.upgrade() {
                editor::resized(&ui, width);
            }
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_send_compose(move || {
            let Some(ui) = weak.upgrade() else { return };
            save_draft(&ui, &tabs);
            let Some(TabContent::Draft(d)) = tabs.borrow().active().map(|t| t.content.clone()) else { return };
            let fail = |message: String| {
                if let Some(Tab { content: TabContent::Draft(d), .. }) = tabs.borrow_mut().active_mut() {
                    d.error = message.clone();
                }
                ui.set_compose_error(message.into());
            };
            if mail::is_spike() {
                return fail(tr!("Cannot send while showing development data").into());
            }
            let Some(account) = mail::account_by_id(&d.account_id) else { return fail(tr!("There is no account to send from").into()) };
            let parse = |field: &str| fuu_core::compose::parse_recipients(field);
            let (to, cc, bcc) = match (parse(&d.to), parse(&d.cc), parse(&d.bcc)) {
                (Ok(a), Ok(b), Ok(c)) => (a, b, c),
                (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => return fail(e),
            };
            if to.is_empty() && cc.is_empty() && bcc.is_empty() {
                return fail(tr!("Enter a recipient").into());
            }
            let outgoing = fuu_core::compose::Outgoing {
                from_name: account.display_name.clone(),
                from_email: account.email.clone(),
                to,
                cc,
                bcc,
                subject: d.subject.clone(),
                body: d.doc.to_plain(),
                attachments: d
                    .attachments
                    .iter()
                    .map(|a| fuu_core::compose::OutgoingAttachment { name: a.name.clone(), mime: a.mime.clone(), data: a.data.clone() })
                    .collect(),
                in_reply_to: d.in_reply_to.clone(),
                references: d.references.clone(),
                html: ui.get_compose_html(),
                html_body: ui.get_compose_html().then(|| d.doc.to_html()),
            };
            if let Some(Tab { content: TabContent::Draft(d), .. }) = tabs.borrow_mut().active_mut() {
                d.sending = true;
                d.error.clear();
            }
            ui.set_compose_sending(true);
            ui.set_compose_error(SharedString::default());
            mail::send_outgoing(&account.id, outgoing, d.answered.clone(), d.id);
            let _ = &state;
        });
    }
    {
        // A draft was sent: close its tab, wherever it is.
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_draft_sent(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            let index = tabs.borrow().items().iter().position(|t| matches!(&t.content, TabContent::Draft(d) if d.id == id as u64));
            if let Some(i) = index {
                request_close(&ui, &state, &tabs, i);
            }
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_draft_failed(move |id, message| {
            let Some(ui) = weak.upgrade() else { return };
            for tab in tabs.borrow_mut().items_mut() {
                if let TabContent::Draft(d) = &mut tab.content {
                    if d.id == id as u64 {
                        d.sending = false;
                        d.error = message.to_string();
                    }
                }
            }
            let active_is_it = matches!(tabs.borrow().active().map(|t| &t.content), Some(TabContent::Draft(d)) if d.id == id as u64);
            if active_is_it {
                ui.set_compose_sending(false);
                ui.set_compose_error(message);
            }
            let _ = &state;
        });
    }
    {
        let (tabs, weak) = (tabs.clone(), ui.as_weak());
        ui.on_compose_add_attachment(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(paths) = rfd::FileDialog::new().pick_files() else { return };
            let mut tabs_mut = tabs.borrow_mut();
            let Some(Tab { content: TabContent::Draft(d), .. }) = tabs_mut.active_mut() else { return };
            let mut errors = Vec::new();
            for path in paths {
                match std::fs::read(&path) {
                    Ok(data) => {
                        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "attachment".into());
                        let mime = fuu_core::compose::guess_mime(&name).to_owned();
                        d.attachments.push(DraftAttachment { name, mime, data });
                    }
                    Err(e) => errors.push(format!("{}: {e}", path.display())),
                }
            }
            let total: usize = d.attachments.iter().map(|a| a.data.len()).sum();
            d.error = if !errors.is_empty() {
                tr!("Could not read: {0}", errors.join(", "))
            } else if total > ATTACHMENT_LIMIT {
                tr!("The attachments total {0}. Some servers will not accept that much", human_size(total as u64))
            } else {
                String::new()
            };
            ui.set_compose_attachments(ModelRc::new(VecModel::from(draft_attachment_items(d))));
            ui.set_compose_error(d.error.clone().into());
        });
    }
    {
        let (tabs, weak) = (tabs.clone(), ui.as_weak());
        ui.on_compose_remove_attachment(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let mut tabs_mut = tabs.borrow_mut();
            let Some(Tab { content: TabContent::Draft(d), .. }) = tabs_mut.active_mut() else { return };
            if (index as usize) < d.attachments.len() {
                d.attachments.remove(index as usize);
            }
            ui.set_compose_attachments(ModelRc::new(VecModel::from(draft_attachment_items(d))));
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_discard_compose(move || {
            if let Some(ui) = weak.upgrade() {
                close_active(&ui, &state, &tabs);
            }
        });
    }
    {
        let pending = pending_link.clone();
        ui.on_link_confirmed(move || {
            if let Some(url) = pending.borrow_mut().take() {
                if let Err(e) = open::that_detached(&url) {
                    eprintln!("could not open {url}: {e}");
                }
            }
        });
    }

    ui.show()?;
    // The native window is created lazily once the event loop runs, so retry
    // briefly until winit hands it out.
    let _native_window_timer = {
        let timer = std::rc::Rc::new(slint::Timer::default());
        let me = std::rc::Rc::downgrade(&timer);
        let weak = ui.as_weak();
        let tries = std::cell::Cell::new(0u32);
        timer.start(slint::TimerMode::Repeated, Duration::from_millis(20), move || {
            tries.set(tries.get() + 1);
            let done = weak.upgrade().is_none_or(|ui| finish_native_window(&ui));
            if done || tries.get() >= 250 {
                if let Some(t) = me.upgrade() {
                    t.stop();
                }
            }
        });
        timer
    };

    // Renderer geometry sync (polling is a spike simplification).
    let sync_timer = slint::Timer::default();
    {
        let weak = ui.as_weak();
        let state = state.clone();
        sync_timer.start(slint::TimerMode::Repeated, Duration::from_millis(16), move || {
            let Some(ui) = weak.upgrade() else { return };

            let maximised = ui.window().is_maximized();
            if maximised != ui.get_maximised() {
                ui.set_maximised(maximised);
            }

            let mut st = state.borrow_mut();
            let Some(r) = st.reader.as_mut() else { return };
            let scale = ui.window().scale_factor();
            let composited = r.renderer.composited_by_host();

            editor::poll(&ui);
            if composited {
                r.renderer.poll_repaint();
                let has_selection = r.renderer.selected_text().is_some();
                if has_selection != ui.get_has_selection() {
                    ui.set_has_selection(has_selection);
                }
                if scale != r.last_scale {
                    r.last_scale = scale;
                    r.renderer.set_scale_factor(scale);
                }
                let dark = ui.get_dark_mode();
                if dark != r.last_dark {
                    r.last_dark = dark;
                    r.renderer.set_dark_mode(dark);
                }
            }

            let width_logical = if composited { ui.get_reader_w() - SCROLLBAR_ALLOWANCE } else { ui.get_reader_w() };
            let rect = Rect {
                x: (ui.get_reader_x() * scale).round() as i32,
                y: (ui.get_reader_y() * scale).round() as i32,
                width: (width_logical * scale).round().max(1.0) as u32,
                height: (ui.get_reader_h() * scale).round().max(1.0) as u32,
            };
            if rect != r.last_rect {
                if composited {
                    // Re-layout only once the size has settled.
                    match r.pending_rect {
                        Some((pending, since)) if pending == rect => {
                            if since.elapsed() >= RELAYOUT_DEBOUNCE || r.last_rect.width == 0 {
                                r.last_rect = rect;
                                r.pending_rect = None;
                                let _ = r.renderer.set_bounds(rect);
                            }
                        }
                        _ => r.pending_rect = Some((rect, Instant::now())),
                    }
                } else {
                    r.last_rect = rect;
                    let _ = r.renderer.set_bounds(rect);
                }
            }

            let visible = composited || !ui.get_reader_covered();
            if visible != r.last_visible {
                r.last_visible = visible;
                let _ = r.renderer.set_visible(visible);
            }

            let stats_line = match &r.stats {
                Some(s) => format!(
                    "{} ｜ 配信 {}・拒否 {}・遷移遮断 {}",
                    r.renderer.name(),
                    s.served.load(Ordering::Relaxed),
                    s.refused.load(Ordering::Relaxed),
                    s.navigations_blocked.load(Ordering::Relaxed)
                ),
                None => format!("{}\n{}", r.renderer.name(), ui.get_renderer_error()),
            };
            let text: SharedString = format!("{}\n{}", ui.get_feed_summary(), stats_line).into();
            if ui.get_status_text() != text {
                ui.set_status_text(text);
            }
        });
    }

    // Development aid: FUU_SNAPSHOT=<file.png> opens a sample, saves a snapshot of the
    // window and exits. FUU_SNAPSHOT_STEP picks the scene (message|security|compose|dialog|empty|sections|…).
    let snapshot_timer = slint::Timer::default();
    if let Ok(path) = std::env::var("FUU_SNAPSHOT") {
        let weak = ui.as_weak();
        let snapshot_tabs = tabs.clone();
        let step = std::env::var("FUU_SNAPSHOT_STEP").unwrap_or_else(|_| "message".into());
        let mut ticks = 0u32;
        snapshot_timer.start(slint::TimerMode::Repeated, Duration::from_millis(500), move || {
            let Some(ui) = weak.upgrade() else { return };
            ticks += 1;
            match ticks {
                3 => match step.as_str() {
                    "security" => ui.invoke_load_security_sample(),
                    "conversation" => {
                        let model = ui.get_messages();
                        if let Some(vec) = model.as_any().downcast_ref::<VecModel<MessageRow>>() {
                            if let Some(row) = vec.row_data(0) {
                                let children = vec![
                                    ConversationRow { sender: "Alex Morgan".into(), subject: "Re: Project".into(), date: "11/17".into(), sent: false },
                                    ConversationRow { sender: "me@example.com".into(), subject: "Re: Project".into(), date: "11/12".into(), sent: true },
                                ];
                                vec.set_row_data(0, MessageRow { conversation_open: true, conversation: ModelRc::new(VecModel::from(children)), ..row });
                            }
                        }
                    }
                    "compose" => {
                        ui.invoke_row_selected(1);
                        ui.invoke_start_compose();
                    }
                    "tabs" => {
                        ui.invoke_row_selected(1);
                        ui.invoke_row_open_in_tab(2);
                        ui.invoke_row_open_in_tab(3);
                        ui.invoke_reply(true);
                    }
                    "setup" => {
                        ui.invoke_open_account_setup();
                        ui.set_setup_email("someone@gmail.com".into());
                        ui.invoke_setup_email_edited("someone@gmail.com".into());
                        ui.set_setup_advanced(true);
                    }
                    "account-menu" => {
                        ui.invoke_row_selected(0);
                        ui.invoke_show_account_menu();
                    }
                    "mailbox-menu" => ui.invoke_show_mailbox_menu(),
                    "settings" => {
                        ui.invoke_row_selected(1);
                        ui.invoke_open_settings();
                    }
                    "language" => {
                        ui.invoke_open_settings();
                        ui.set_settings_page(5);
                    }
                    "language-switch" => {
                        ui.invoke_open_settings();
                        ui.set_settings_page(5);
                        ui.set_language_index(3);
                    }
                    "update" => {
                        ui.invoke_row_selected(1);
                        ui.invoke_open_settings();
                        ui.set_settings_page(0);
                        ui.set_update_version("0.1.0-alpha.3".into());
                        ui.set_update_state(update::state::AVAILABLE);
                        ui.set_update_status("新しいバージョン 0.1.0-alpha.3 があります。".into());
                    }
                    "accounts" => {
                        ui.invoke_row_selected(1);
                        ui.invoke_open_settings();
                        ui.set_settings_page(1);
                    }
                    "edit-account" => {
                        ui.invoke_row_selected(1);
                        ui.invoke_edit_account("spike".into());
                    }
                    "signature" => {
                        ui.invoke_row_selected(1);
                        ui.invoke_open_settings();
                        ui.set_settings_page(2);
                    }
                    "about" => {
                        ui.invoke_row_selected(1);
                        ui.invoke_open_settings();
                        ui.set_settings_page(4);
                    }
                    "dialog" => {
                        ui.invoke_row_selected(1);
                        ui.set_info_dialog_open(true);
                    }
                    "rich" => {
                        ui.invoke_row_selected(1);
                        ui.invoke_reply(false);
                    }
                    "reply" => {
                        ui.invoke_row_selected(1);
                        ui.invoke_reply(false);
                    }
                    "tooltip" => ui.invoke_row_selected(1),
                    "reauth" => {
                        ui.set_reauth_email("someone@gmail.com".into());
                        ui.set_reauth_error("再ログインが必要です（invalid_grant: Token has been expired or revoked.）".into());
                    }
                    "notice" => {
                        ui.invoke_row_selected(1);
                        notify(&ui, "サーバーに接続できません: imap.example.jp:993 (connection refused)", true);
                    }
                    "empty" => {}
                    "sections" => {
                        // Close every section but today's, as a user tidying up would.
                        let model = ui.get_messages();
                        let mut names: Vec<SharedString> = model.iter().filter(|r| r.header).map(|r| r.section).collect();
                        names.dedup();
                        let started = Instant::now();
                        for name in names.iter().filter(|n| n.as_str() != "今日") {
                            ui.invoke_toggle_section(name.clone());
                        }
                        eprintln!("snapshot: closed {} sections in {:?}", names.len() - 1, started.elapsed());
                        ui.invoke_row_selected(1);
                    }
                    _ => ui.invoke_row_selected(1),
                },
                4 if step == "rich" => editor::load(&ui, editor::sample()),
                // FUU_SNAPSHOT_ATTACH=<file>: attach the file to the shown message and preview it.
                4 if step == "attachment" => {
                    let file = std::env::var("FUU_SNAPSHOT_ATTACH").unwrap_or_default();
                    let Ok(data) = std::fs::read(&file) else { return };
                    let name = std::path::Path::new(&file).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let Some(TabContent::Message(r)) = snapshot_tabs.borrow().active().map(|t| t.content.clone()) else { return };
                    let outgoing = fuu_core::compose::Outgoing {
                        from_email: "spike@fuu.invalid".into(),
                        to: vec![fuu_core::compose::Recipient { name: None, email: "spike@fuu.invalid".into() }],
                        subject: r.subject.clone(),
                        body: "添付ファイルのテスト".into(),
                        attachments: vec![fuu_core::compose::OutgoingAttachment { name, mime: "application/octet-stream".into(), data }],
                        ..Default::default()
                    };
                    match fuu_core::compose::build(&outgoing) {
                        Ok((_, raw)) => {
                            mail::insert_body(&r, raw);
                            ui.invoke_open_attachment(0, false);
                        }
                        Err(e) => eprintln!("snapshot attachment: {e}"),
                    }
                }
                4 if step == "tooltip" => {
                    let tip = ui.global::<Tooltip>();
                    tip.set_text(tr!("Reply all").into());
                    tip.set_x(1000.0);
                    tip.set_y(170.0);
                    tip.set_shown(true);
                }
                5 if step == "rich" => {
                    editor::focus(&ui);
                    editor::pointer(&ui, 0, 30.0, 60.0, 2, false);
                }
                n if n == if step == "attachment" { 16 } else { 6 } => {
                    {
                        use slint::winit_030::WinitWindowAccessor;
                        let decorated = ui.window().with_winit_window(|w| w.is_decorated());
                        eprintln!("snapshot: native decorations = {decorated:?}");
                    }
                    match ui.window().take_snapshot() {
                        Ok(buf) => {
                            let file = std::fs::File::create(&path).expect("snapshot file");
                            let mut enc = png::Encoder::new(std::io::BufWriter::new(file), buf.width(), buf.height());
                            enc.set_color(png::ColorType::Rgba);
                            enc.set_depth(png::BitDepth::Eight);
                            let mut writer = enc.write_header().expect("png header");
                            writer.write_image_data(buf.as_bytes()).expect("png data");
                        }
                        Err(e) => eprintln!("snapshot failed: {e}"),
                    }
                    let _ = slint::quit_event_loop();
                }
                _ => {}
            }
        });
    }

    // Dummy data for UI development; the real engine otherwise.
    // Snapshot runs use the dummy data unless FUU_SNAPSHOT_REAL is set.
    let spike = std::env::var_os("FUU_SPIKE_FEED").is_some()
        || (std::env::var_os("FUU_SNAPSHOT").is_some() && std::env::var_os("FUU_SNAPSHOT_REAL").is_none());
    mail::start(&ui, spike);
    if spike {
        spawn_mail_feed(ui.as_weak());
    }
    slint::run_event_loop()
}
