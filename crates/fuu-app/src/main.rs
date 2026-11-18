// Fuu — minimal desktop mailer. M0 spike: Slint shell, swappable body renderers
// (Blitz composited image / hardened child webview / text), tokio → Slint model
// bridge, IME and CJK checks.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod samples;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use fuu_model::{Rect, RemoteContentPolicy, SafeDocument};
use fuu_render::blitz::{BlitzRenderer, RenderedPage};
use fuu_render::text::TextRenderer;
use fuu_render::webview::WebViewRenderer;
use fuu_render::{BodyRenderer, LinkHandler, LinkRequest, RenderStats};
use slint::{ComponentHandle, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};

slint::include_modules!();

const SPIKE_ROWS: usize = 5000;
const BATCH: usize = 500;
/// Logical width reserved for the reader's vertical scrollbar.
const SCROLLBAR_ALLOWANCE: f32 = 16.0;
/// Re-layout delay for host-composited engines while the pane is being resized.
const RELAYOUT_DEBOUNCE: Duration = Duration::from_millis(120);

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
                ui.set_compose_to(addr.into());
                ui.set_compose_subject(SharedString::default());
                ui.set_compose_body(SharedString::default());
                ui.set_composing(true);
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

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    ui.set_messages(ModelRc::new(VecModel::<MessageRow>::default()));
    ui.set_feed_summary("読み込み中…".into());

    let state: SharedState = Rc::new(RefCell::new(ReaderState::default()));
    let wanted_engine = Rc::new(RefCell::new(Engine::from_env()));
    let pending_link: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_row_selected(move |i| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(row) = ui.get_messages().row_data(i as usize) else { return };
            ui.set_reader_subject(row.subject.clone());
            ui.set_reader_from(format!("{} <sender{}@example.com>", row.sender, i).into());
            ui.set_reader_date(format!("2026/{} 10:{:02}", row.date, i % 60).into());
            let doc = samples::message_document(i as usize, &row.subject);
            ui.set_fallback_text(doc.text.clone().into());
            show_document(&state, doc);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_load_security_sample(move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_current_row(-1);
            ui.set_reader_subject("セキュリティ検査文書".into());
            ui.set_reader_from("M0 spike <spike@fuu.invalid>".into());
            ui.set_reader_date(SharedString::default());
            let doc = samples::security_document();
            ui.set_fallback_text(doc.text.clone().into());
            show_document(&state, doc);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_load_cjk_sample(move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_current_row(-1);
            ui.set_reader_subject("CJK：日本語・简体・繁體・한국어 ✉️".into());
            ui.set_reader_from("髙﨑 𠮷子 <takasaki@example.jp>".into());
            ui.set_reader_date(SharedString::default());
            let doc = samples::cjk_document();
            ui.set_fallback_text(doc.text.clone().into());
            show_document(&state, doc);
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
                r.renderer.pointer_click(x * scale, y * scale);
                state.borrow_mut().reader = Some(r);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_page_pointer_moved(move |x, y| {
            let Some(ui) = weak.upgrade() else { return };
            let scale = ui.window().scale_factor();
            let hover = state
                .borrow()
                .reader
                .as_ref()
                .is_some_and(|r| r.renderer.link_at(x * scale, y * scale));
            if hover != ui.get_page_link_hover() {
                ui.set_page_link_hover(hover);
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
        let weak = ui.as_weak();
        ui.on_start_compose(move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_compose_to(SharedString::default());
            ui.set_compose_subject(SharedString::default());
            ui.set_compose_body("\n\n-- \n署名（M3 でアカウントごとに設定）".into());
            ui.set_composing(true);
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_send_compose(move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_feed_summary(format!("送信は M3 で実装（本文 {} 文字）", ui.get_compose_body().chars().count()).into());
            ui.set_composing(false);
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

            let mut st = state.borrow_mut();
            let Some(r) = st.reader.as_mut() else { return };
            let scale = ui.window().scale_factor();
            let composited = r.renderer.composited_by_host();

            if composited {
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

    spawn_mail_feed(ui.as_weak());
    slint::run_event_loop()
}
