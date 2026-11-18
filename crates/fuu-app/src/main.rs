// Fuu — minimal desktop mailer. M0 spike: Slint shell, swappable body renderers
// (Blitz composited image / hardened child webview / text), tokio → Slint model
// bridge, IME and CJK checks.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod samples;
mod tabs;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use fuu_model::{Rect, RemoteContentPolicy, SafeDocument};
use fuu_render::blitz::{BlitzRenderer, RenderedPage};
use fuu_render::text::TextRenderer;
use fuu_render::webview::WebViewRenderer;
use fuu_render::{BodyRenderer, LinkHandler, LinkRequest, PointerPhase, RenderStats};
use slint::{ComponentHandle, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};
use tabs::{Draft, Tab, TabContent, Tabs};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Engine {
    Blitz,
    WebView,
}

impl Engine {
    fn from_env() -> Self {
        match std::env::var("FUU_RENDERER").as_deref() {
            Ok("webview") => Engine::WebView,
            _ => Engine::Blitz,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Engine::Blitz => "Blitz",
            Engine::WebView => "WebView",
        }
    }

    fn other(self) -> Self {
        match self {
            Engine::Blitz => Engine::WebView,
            Engine::WebView => Engine::Blitz,
        }
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
    engine: Option<Engine>,
    /// The document on screen, re-shown when the engine is switched.
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
            for batch in 0..batches {
                tokio::time::sleep(Duration::from_millis(60)).await;
                let rows: Vec<(String, String, String, String, bool)> = (0..BATCH)
                    .map(|j| {
                        let n = batch * BATCH + j;
                        (
                            ["佐藤 花子", "Oliver Smith", "鈴木一郎", "Newsletter", "山田 太郎"][n % 5].to_owned(),
                            format!("［#{n}］打ち合わせの件について — Re: colour proposal"),
                            "お疲れさまです。先日の件ですが、資料を添付しましたのでご確認ください。".to_owned(),
                            format!("11/{:02}", 18 - (n / 300).min(17)),
                            n % 7 == 0,
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
                    vec.extend(rows.into_iter().map(|(sender, subject, preview, date, unread)| MessageRow {
                        initials: initials(&sender).into(),
                        avatar: avatar_index(&sender),
                        sender: sender.into(),
                        subject: subject.into(),
                        preview: preview.into(),
                        date: date.into(),
                        unread,
                    }));
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
            LinkRequest::Mailto(url) => {
                let addr = url["mailto:".len()..].split('?').next().unwrap_or_default().to_owned();
                ui.invoke_compose_to_address(addr.into());
            }
        }
    })
}

fn text_reader(ui: &AppWindow, why: String) -> Reader {
    eprintln!("{why}");
    ui.set_renderer_error(why.into());
    ui.set_use_page_image(false);
    ui.set_use_fallback(true);
    let sink_ui = ui.as_weak();
    let sink = Box::new(move |text: &str| {
        if let Some(ui) = sink_ui.upgrade() {
            ui.set_fallback_text(text.into());
        }
    });
    new_reader(Box::new(TextRenderer::new(sink)), None)
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

fn create_reader(ui: &AppWindow, engine: Engine, pending_link: Rc<RefCell<Option<String>>>) -> Reader {
    ui.set_renderer_label(engine.label().into());
    ui.set_renderer_error(SharedString::default());
    ui.set_use_fallback(false);
    let on_link = link_handler(ui, pending_link);
    match engine {
        Engine::Blitz => {
            ui.set_use_page_image(true);
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
        Engine::WebView => {
            ui.set_use_page_image(false);
            let handle = ui.window().window_handle();
            match WebViewRenderer::new(&handle, Rect::default(), on_link) {
                Ok(webview) => {
                    let stats = webview.stats();
                    new_reader(Box::new(webview), Some(stats))
                }
                Err(e) => text_reader(ui, format!("webview unavailable, using text renderer: {e}")),
            }
        }
    }
}

type SharedTabs = Rc<RefCell<Tabs>>;

const TAB_LIMIT_MESSAGE: &str = "タブは最大 32 個までです";

/// The signature block appended to new drafts, from the settings.
fn signature(ui: &AppWindow) -> String {
    let text = ui.get_signature_text();
    if ui.get_signature_enabled() && !text.trim().is_empty() {
        format!("\n\n-- \n{text}")
    } else {
        String::new()
    }
}

fn tab_title(ui: &AppWindow, tab: &Tab) -> String {
    match &tab.content {
        TabContent::Message(i) => ui.get_messages().row_data(*i).map(|r| r.subject.to_string()).unwrap_or_else(|| "メール".into()),
        TabContent::SecuritySample => "セキュリティ検査文書".into(),
        TabContent::CjkSample => "CJK 表示サンプル".into(),
        TabContent::Draft(d) => if d.subject.trim().is_empty() { d.title.clone() } else { d.subject.clone() },
        TabContent::Settings => "設定".into(),
    }
}

fn tab_kind(tab: &Tab) -> i32 {
    match tab.content {
        TabContent::Draft(_) => 1,
        TabContent::Settings => 2,
        _ => 0,
    }
}

fn render_tabs(ui: &AppWindow, tabs: &Tabs) {
    let items: Vec<TabItem> = tabs
        .items()
        .iter()
        .map(|t| TabItem { title: tab_title(ui, t).into(), kind: tab_kind(t), unseen: t.unseen })
        .collect();
    ui.set_tabs(ModelRc::new(VecModel::from(items)));
    ui.set_active_tab(tabs.active_index().map_or(-1, |i| i as i32));
}

/// Copy the composer fields back into the active draft tab.
fn save_draft(ui: &AppWindow, tabs: &SharedTabs) {
    if let Some(Tab { content: TabContent::Draft(d), .. }) = tabs.borrow_mut().active_mut() {
        d.from = ui.get_compose_from().into();
        d.to = ui.get_compose_to().into();
        d.cc = ui.get_compose_cc().into();
        d.bcc = ui.get_compose_bcc().into();
        d.subject = ui.get_compose_subject().into();
        d.body = ui.get_compose_body().into();
        d.show_cc_bcc = ui.get_compose_show_cc_bcc();
    }
}

/// Show the active tab in the right pane (or the empty state when there is none).
fn apply_active(ui: &AppWindow, state: &SharedState, tabs: &SharedTabs) {
    let content = tabs.borrow().active().map(|t| t.content.clone());
    ui.set_composing(false);
    ui.set_settings_open(false);
    ui.set_current_row(-1);
    ui.set_has_message(false);
    let doc = match content {
        None => None,
        Some(TabContent::Message(i)) => ui.get_messages().row_data(i).map(|row| {
            if row.unread {
                if let Some(vec) = ui.get_messages().as_any().downcast_ref::<VecModel<MessageRow>>() {
                    vec.set_row_data(i, MessageRow { unread: false, ..row.clone() });
                }
            }
            ui.set_current_row(i as i32);
            set_reader_header(ui, &row.subject, &row.sender, &format!("sender{i}@example.com"), &format!("2026/{} 10:{:02}", row.date, i % 60));
            samples::message_document(i, &row.subject)
        }),
        Some(TabContent::SecuritySample) => {
            set_reader_header(ui, "セキュリティ検査文書", "M0 spike", "spike@fuu.invalid", "");
            Some(samples::security_document())
        }
        Some(TabContent::CjkSample) => {
            set_reader_header(ui, "CJK：日本語・简体・繁體・한국어 ✉️", "髙﨑 𠮷子", "takasaki@example.jp", "");
            Some(samples::cjk_document())
        }
        Some(TabContent::Draft(d)) => {
            ui.set_compose_title(d.title.into());
            ui.set_compose_from(d.from.into());
            ui.set_compose_to(d.to.into());
            ui.set_compose_cc(d.cc.into());
            ui.set_compose_bcc(d.bcc.into());
            ui.set_compose_subject(d.subject.into());
            ui.set_compose_body(d.body.into());
            ui.set_compose_show_cc_bcc(d.show_cc_bcc);
            ui.set_composing(true);
            None
        }
        Some(TabContent::Settings) => {
            ui.set_settings_open(true);
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

/// Show a viewer (message or sample) in the active viewer tab, or a new tab.
fn show_viewer(ui: &AppWindow, state: &SharedState, tabs: &SharedTabs, content: TabContent) {
    save_draft(ui, tabs);
    if tabs.borrow_mut().show(content).is_err() {
        ui.set_feed_summary(TAB_LIMIT_MESSAGE.into());
        return;
    }
    apply_active(ui, state, tabs);
}

/// Open a draft in a new tab.
fn open_draft(ui: &AppWindow, state: &SharedState, tabs: &SharedTabs, mut draft: Draft) {
    save_draft(ui, tabs);
    draft.from = format!("{} <{}>", ui.get_account_name(), ui.get_account_email());
    draft.show_cc_bcc = draft.show_cc_bcc || !draft.cc.is_empty() || !draft.bcc.is_empty();
    if tabs.borrow_mut().open(TabContent::Draft(draft), true).is_err() {
        ui.set_feed_summary(TAB_LIMIT_MESSAGE.into());
        return;
    }
    apply_active(ui, state, tabs);
}

/// Close the active tab (used by send, discard and Ctrl/Cmd+W).
fn close_active(ui: &AppWindow, state: &SharedState, tabs: &SharedTabs) {
    save_draft(ui, tabs);
    let active = tabs.borrow().active_index();
    if let Some(i) = active {
        tabs.borrow_mut().close(i);
    }
    apply_active(ui, state, tabs);
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
                attributes.with_decorations(false)
            }
        })
        .select()
}

fn install_window_chrome(ui: &AppWindow) {
    use slint::winit_030::winit::window::ResizeDirection;
    use slint::winit_030::WinitWindowAccessor;

    ui.set_is_mac(cfg!(target_os = "macos"));
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

fn main() -> Result<(), slint::PlatformError> {
    select_backend()?;
    let ui = AppWindow::new()?;
    install_window_chrome(&ui);
    ui.set_messages(ModelRc::new(VecModel::<MessageRow>::default()));
    ui.set_feed_summary("読み込み中…".into());

    let state: SharedState = Rc::new(RefCell::new(ReaderState::default()));
    let wanted_engine = Rc::new(RefCell::new(Engine::from_env()));
    let pending_link: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let tabs: SharedTabs = Rc::new(RefCell::new(Tabs::default()));
    render_tabs(&ui, &tabs.borrow());

    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_row_selected(move |i| {
            if let Some(ui) = weak.upgrade() {
                show_viewer(&ui, &state, &tabs, TabContent::Message(i as usize));
            }
        });
    }
    {
        let (tabs, weak) = (tabs.clone(), ui.as_weak());
        ui.on_row_open_in_tab(move |i| {
            let Some(ui) = weak.upgrade() else { return };
            save_draft(&ui, &tabs);
            if tabs.borrow_mut().open(TabContent::Message(i as usize), false).is_err() {
                ui.set_feed_summary(TAB_LIMIT_MESSAGE.into());
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
            if tabs.borrow_mut().show_settings().is_err() {
                ui.set_feed_summary(TAB_LIMIT_MESSAGE.into());
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
            save_draft(&ui, &tabs);
            tabs.borrow_mut().activate(i as usize);
            apply_active(&ui, &state, &tabs);
        });
    }
    {
        let (tabs, weak) = (tabs.clone(), ui.as_weak());
        ui.on_tab_moved(move |from, to| {
            let Some(ui) = weak.upgrade() else { return };
            tabs.borrow_mut().move_tab(from as usize, to as usize);
            render_tabs(&ui, &tabs.borrow());
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_tab_closed(move |i| {
            let Some(ui) = weak.upgrade() else { return };
            save_draft(&ui, &tabs);
            tabs.borrow_mut().close(i as usize);
            apply_active(&ui, &state, &tabs);
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
                Ok(()) => ui.set_feed_summary(format!("{} 文字をコピー", text.chars().count()).into()),
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
        let wanted = wanted_engine.clone();
        ui.on_toggle_renderer(move || {
            let next = wanted.borrow().other();
            *wanted.borrow_mut() = next;
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_start_compose(move || {
            if let Some(ui) = weak.upgrade() {
                let draft = Draft { title: "新規メッセージ".into(), body: signature(&ui), ..Draft::default() };
                open_draft(&ui, &state, &tabs, draft);
            }
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_compose_to_address(move |address| {
            if let Some(ui) = weak.upgrade() {
                let draft = Draft { title: "新規メッセージ".into(), to: address.into(), body: signature(&ui), ..Draft::default() };
                open_draft(&ui, &state, &tabs, draft);
            }
        });
    }
    {
        // Spike replies: real quoting, recipients and threading headers land in M3.
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_reply(move |all| {
            let Some(ui) = weak.upgrade() else { return };
            let subject = ui.get_reader_subject();
            let subject = if subject.starts_with("Re:") { subject.to_string() } else { format!("Re: {subject}") };
            let draft = Draft {
                title: if all { "全員に返信" } else { "返信" }.into(),
                to: ui.get_reader_address().into(),
                cc: if all { "team@example.com".into() } else { String::new() },
                subject,
                body: format!("{}\n\n> （引用は M3 で実装）", signature(&ui)),
                ..Draft::default()
            };
            open_draft(&ui, &state, &tabs, draft);
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_forward(move || {
            let Some(ui) = weak.upgrade() else { return };
            let draft = Draft {
                title: "転送".into(),
                subject: format!("Fwd: {}", ui.get_reader_subject()),
                body: signature(&ui),
                ..Draft::default()
            };
            open_draft(&ui, &state, &tabs, draft);
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_archive(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_feed_summary("アーカイブは M2 で実装".into());
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_delete(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_feed_summary("削除は M2 で実装".into());
            }
        });
    }
    {
        let (state, tabs, weak) = (state.clone(), tabs.clone(), ui.as_weak());
        ui.on_send_compose(move || {
            let Some(ui) = weak.upgrade() else { return };
            let recipients = [ui.get_compose_to(), ui.get_compose_cc(), ui.get_compose_bcc()]
                .iter()
                .flat_map(|f| f.split(',').map(str::trim).filter(|a| !a.is_empty()).map(str::to_owned).collect::<Vec<_>>())
                .count();
            ui.set_feed_summary(format!("送信は M3 で実装（宛先 {recipients} 件・本文 {} 文字）", ui.get_compose_body().chars().count()).into());
            close_active(&ui, &state, &tabs);
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

    // Engine lifecycle and geometry sync. The webview needs the native winit
    // window, which is realised asynchronously after the event loop starts, so
    // engines are (re)created on a tick once the handle exists. Polling is a spike
    // simplification.
    let sync_timer = slint::Timer::default();
    {
        let weak = ui.as_weak();
        let state = state.clone();
        let wanted = wanted_engine.clone();
        let pending = pending_link.clone();
        let started = Instant::now();
        sync_timer.start(slint::TimerMode::Repeated, Duration::from_millis(16), move || {
            use raw_window_handle::HasWindowHandle;
            let Some(ui) = weak.upgrade() else { return };
            let wanted = *wanted.borrow();

            if state.borrow().engine != Some(wanted) {
                let realised = ui.window().window_handle().window_handle().is_ok();
                if !realised && started.elapsed() < Duration::from_secs(3) {
                    return;
                }
                // Drop the old engine first (removes a native child webview).
                let current = {
                    let mut st = state.borrow_mut();
                    st.reader = None;
                    st.current.clone()
                };
                let mut reader = create_reader(&ui, wanted, pending.clone());
                if let Some(doc) = &current {
                    let _ = reader.renderer.show(doc, RemoteContentPolicy::Block);
                }
                let mut st = state.borrow_mut();
                st.reader = Some(reader);
                st.engine = Some(wanted);
            }

            let maximised = ui.window().is_maximized();
            if maximised != ui.get_maximised() {
                ui.set_maximised(maximised);
            }

            let mut st = state.borrow_mut();
            let Some(r) = st.reader.as_mut() else { return };
            let scale = ui.window().scale_factor();
            let composited = r.renderer.composited_by_host();

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
    // window and exits. FUU_SNAPSHOT_STEP picks the scene (message|security|compose|dialog|empty).
    let snapshot_timer = slint::Timer::default();
    if let Ok(path) = std::env::var("FUU_SNAPSHOT") {
        let weak = ui.as_weak();
        let step = std::env::var("FUU_SNAPSHOT_STEP").unwrap_or_else(|_| "message".into());
        let mut ticks = 0u32;
        snapshot_timer.start(slint::TimerMode::Repeated, Duration::from_millis(500), move || {
            let Some(ui) = weak.upgrade() else { return };
            ticks += 1;
            match ticks {
                3 => match step.as_str() {
                    "security" => ui.invoke_load_security_sample(),
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
                    "settings" => {
                        ui.invoke_row_selected(1);
                        ui.invoke_open_settings();
                    }
                    "dialog" => {
                        ui.invoke_row_selected(1);
                        ui.set_info_dialog_open(true);
                    }
                    "empty" => {}
                    _ => ui.invoke_row_selected(1),
                },
                6 => {
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

    spawn_mail_feed(ui.as_weak());
    slint::run_event_loop()
}
