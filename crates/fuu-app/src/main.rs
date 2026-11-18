// Fuu — minimal desktop mailer. M0 spike: Slint shell, hardened child webview,
// tokio → Slint model bridge, IME and CJK checks.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod samples;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use fuu_model::{Rect, RemoteContentPolicy, SafeDocument};
use fuu_render::text::TextRenderer;
use fuu_render::webview::{RenderStats, WebViewRenderer};
use fuu_render::{BodyRenderer, LinkRequest};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

slint::include_modules!();

const SPIKE_ROWS: usize = 5000;
const BATCH: usize = 500;

struct Reader {
    renderer: Box<dyn BodyRenderer>,
    stats: Option<Arc<RenderStats>>,
    last_rect: Rect,
    last_visible: bool,
}

type SharedReader = Rc<RefCell<Option<Reader>>>;

fn show_document(reader: &SharedReader, doc: &SafeDocument) {
    if let Some(r) = reader.borrow_mut().as_mut() {
        if let Err(e) = r.renderer.show(doc, RemoteContentPolicy::Block) {
            eprintln!("render failed: {e}");
        }
    }
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

fn create_reader(ui: &AppWindow, pending_link: Rc<RefCell<Option<String>>>) -> Reader {
    let link_ui = ui.as_weak();
    let on_link = Box::new(move |link: LinkRequest| {
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
    });
    let handle = ui.window().window_handle();
    match WebViewRenderer::new(&handle, Rect::default(), on_link) {
        Ok(webview) => {
            let stats = webview.stats();
            Reader { renderer: Box::new(webview), stats: Some(stats), last_rect: Rect::default(), last_visible: true }
        }
        Err(e) => {
            let msg = format!("webview unavailable, using text renderer: {e}");
            eprintln!("{msg}");
            ui.set_renderer_error(msg.into());
            ui.set_use_fallback(true);
            let sink_ui = ui.as_weak();
            let sink = Box::new(move |text: &str| {
                if let Some(ui) = sink_ui.upgrade() {
                    ui.set_fallback_text(text.into());
                }
            });
            Reader { renderer: Box::new(TextRenderer::new(sink)), stats: None, last_rect: Rect::default(), last_visible: true }
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    ui.set_messages(ModelRc::new(VecModel::<MessageRow>::default()));
    ui.set_feed_summary("読み込み中…".into());

    let reader: SharedReader = Rc::new(RefCell::new(None));
    let pending_link: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

    {
        let reader = reader.clone();
        let weak = ui.as_weak();
        ui.on_row_selected(move |i| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(row) = ui.get_messages().row_data(i as usize) else { return };
            ui.set_reader_subject(row.subject.clone());
            ui.set_reader_from(format!("{} <sender{}@example.com>", row.sender, i).into());
            ui.set_reader_date(format!("2026/{} 10:{:02}", row.date, i % 60).into());
            let doc = samples::message_document(i as usize, &row.subject);
            ui.set_fallback_text(doc.text.clone().into());
            show_document(&reader, &doc);
        });
    }
    {
        let reader = reader.clone();
        let weak = ui.as_weak();
        ui.on_load_security_sample(move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_current_row(-1);
            ui.set_reader_subject("セキュリティ検査文書".into());
            ui.set_reader_from("M0 spike <spike@fuu.invalid>".into());
            ui.set_reader_date(SharedString::default());
            let doc = samples::security_document();
            ui.set_fallback_text(doc.text.clone().into());
            show_document(&reader, &doc);
        });
    }
    {
        let reader = reader.clone();
        let weak = ui.as_weak();
        ui.on_load_cjk_sample(move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_current_row(-1);
            ui.set_reader_subject("CJK：日本語・简体・繁體・한국어 ✉️".into());
            ui.set_reader_from("髙﨑 𠮷子 <takasaki@example.jp>".into());
            ui.set_reader_date(SharedString::default());
            let doc = samples::cjk_document();
            ui.set_fallback_text(doc.text.clone().into());
            show_document(&reader, &doc);
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

    // Keep the child webview aligned with the reader pane and hidden while
    // Slint-drawn overlays cover it. Polling is a spike simplification.
    //
    // The webview is attached on the first tick at which the native winit window
    // exists (it is realised asynchronously after the event loop starts). If no
    // handle appears within the grace period, the text renderer takes over.
    let sync_timer = slint::Timer::default();
    {
        let weak = ui.as_weak();
        let reader = reader.clone();
        let pending = pending_link.clone();
        let started = Instant::now();
        sync_timer.start(slint::TimerMode::Repeated, Duration::from_millis(16), move || {
            use raw_window_handle::HasWindowHandle;
            let Some(ui) = weak.upgrade() else { return };
            let mut guard = reader.borrow_mut();
            if guard.is_none() {
                let realised = ui.window().window_handle().window_handle().is_ok();
                if !realised && started.elapsed() < Duration::from_secs(3) {
                    return;
                }
                *guard = Some(create_reader(&ui, pending.clone()));
            }
            let Some(r) = guard.as_mut() else { return };
            let scale = ui.window().scale_factor();
            let rect = Rect {
                x: (ui.get_reader_x() * scale).round() as i32,
                y: (ui.get_reader_y() * scale).round() as i32,
                width: (ui.get_reader_w() * scale).round().max(1.0) as u32,
                height: (ui.get_reader_h() * scale).round().max(1.0) as u32,
            };
            if rect != r.last_rect {
                r.last_rect = rect;
                let _ = r.renderer.set_bounds(rect);
            }
            let visible = !ui.get_reader_covered();
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
                None => format!("{}（WebView なし）
{}", r.renderer.name(), ui.get_renderer_error()),
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
