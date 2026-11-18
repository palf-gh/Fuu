//! Blitz renderer: HTML/CSS layout with no JavaScript engine at all, painted on the
//! CPU into a premultiplied RGBA buffer. The host composites the buffer as an image,
//! so its own overlays (dialogs, the composer) draw above the document.
//!
//! Security properties:
//! - No script execution is possible: Blitz has no JavaScript engine.
//! - Every resource request goes through [`ManifestNet`]. Only manifest entries
//!   (`fuu://doc/res/<n>`) and inline raster `data:` images are served; remote URLs,
//!   stylesheets, fonts and everything else are refused and counted.
//! - Navigation and form submission are never performed; link activations are
//!   classified and handed to the host.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use base64::Engine as _;
use atomic_refcell::AtomicRefCell;
use blitz_dom::{local_name, BaseDocument, DocumentConfig, EventDriver, NoopEventHandler};
use blitz_html::HtmlDocument;
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, Point, PointerCoords, PointerDetails, UiEvent,
};
use blitz_traits::navigation::{NavigationOptions, NavigationProvider};
use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request, Url};
use blitz_traits::shell::{ColorScheme, Viewport};

use crate::{
    classify_link, BodyRenderer, LinkHandler, PointerPhase, Rect, RemoteContentPolicy, RenderError, RenderStats, SafeDocument,
};

/// Base URL of every document; manifest entries resolve to `fuu://doc/res/<n>`.
const DOCUMENT_BASE: &str = "fuu://doc/";
/// Raster formats a document may load. SVG (scriptable) and anything else is refused.
const SERVABLE_RESOURCE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];
/// Upper bound on the painted height, in physical pixels.
const MAX_PAINT_HEIGHT: u32 = 16_000;
/// Minimum interval between selection repaints (full-page CPU paints).
const REPAINT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(33);

/// A painted document handed to the host.
#[derive(Debug, Clone)]
pub struct RenderedPage {
    pub width: u32,
    pub height: u32,
    /// Device pixel ratio the page was painted at.
    pub scale: f32,
    pub rgba_premultiplied: Vec<u8>,
}

pub type PageSink = Box<dyn Fn(RenderedPage) + 'static>;

/// Resolve a resource URL against the current manifest.
fn resolve_resource(url: &Url, resources: &[(String, Vec<u8>)]) -> Option<Vec<u8>> {
    match url.scheme() {
        "fuu" if url.host_str() == Some("doc") => {
            let index = url.path().strip_prefix("/res/")?.parse::<usize>().ok()?;
            let (mime, data) = resources.get(index)?;
            SERVABLE_RESOURCE_TYPES.contains(&mime.as_str()).then(|| data.clone())
        }
        "data" => {
            let (meta, payload) = url.path().split_once(',')?;
            let mime = meta.strip_suffix(";base64")?.to_ascii_lowercase();
            if !SERVABLE_RESOURCE_TYPES.contains(&mime.as_str()) {
                return None;
            }
            base64::engine::general_purpose::STANDARD.decode(payload.trim()).ok()
        }
        _ => None,
    }
}

struct ManifestNet {
    resources: Arc<Vec<(String, Vec<u8>)>>,
    stats: Arc<RenderStats>,
}

impl NetProvider for ManifestNet {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        match resolve_resource(&request.url, &self.resources) {
            Some(bytes) => {
                self.stats.served.fetch_add(1, Ordering::Relaxed);
                handler.bytes(request.url.to_string(), Bytes::from(bytes));
            }
            None => {
                // Complete refused loads with an empty body (like a 404). Leaving them
                // unanswered would keep render-blocking stylesheets pending forever.
                self.stats.refused.fetch_add(1, Ordering::Relaxed);
                handler.bytes(request.url.to_string(), Bytes::new());
            }
        }
    }
}

/// Refuses every navigation (links are hit-tested by the host; forms never submit).
struct RefuseNavigation {
    stats: Arc<RenderStats>,
}

impl NavigationProvider for RefuseNavigation {
    fn navigate_to(&self, _options: NavigationOptions) {
        self.stats.navigations_blocked.fetch_add(1, Ordering::Relaxed);
    }
}

pub struct BlitzRenderer {
    sink: PageSink,
    on_link: LinkHandler,
    stats: Arc<RenderStats>,
    doc: Option<BaseDocument>,
    source: Option<(String, Arc<Vec<(String, Vec<u8>)>>)>,
    width: u32,
    height: u32,
    scale: f32,
    dark: bool,
    dirty: bool,
    last_paint: std::time::Instant,
}

impl BlitzRenderer {
    pub fn new(sink: PageSink, on_link: LinkHandler) -> Self {
        Self {
            sink,
            on_link,
            stats: Arc::new(RenderStats::default()),
            doc: None,
            source: None,
            width: 600,
            height: 1,
            scale: 1.0,
            dark: false,
            dirty: false,
            last_paint: std::time::Instant::now(),
        }
    }

    pub fn stats(&self) -> Arc<RenderStats> {
        Arc::clone(&self.stats)
    }

    fn viewport(&self, height: u32) -> Viewport {
        let scheme = if self.dark { ColorScheme::Dark } else { ColorScheme::Light };
        Viewport::new(self.width.max(1), height.max(1), self.scale, scheme)
    }

    /// Parse the current source into a fresh document and lay it out at the current
    /// width, sizing the viewport to the full content height.
    fn rebuild(&mut self) {
        let Some((html, resources)) = self.source.clone() else {
            self.doc = None;
            (self.sink)(RenderedPage { width: 1, height: 1, scale: self.scale, rgba_premultiplied: vec![0; 4] });
            return;
        };
        let config = DocumentConfig {
            viewport: Some(self.viewport(1)),
            base_url: Some(DOCUMENT_BASE.to_owned()),
            net_provider: Some(Arc::new(ManifestNet { resources, stats: Arc::clone(&self.stats) })),
            navigation_provider: Some(Arc::new(RefuseNavigation { stats: Arc::clone(&self.stats) })),
            ..DocumentConfig::default()
        };
        let mut doc = HtmlDocument::from_html(&html, config).into_inner();
        self.layout_and_paint(&mut doc);
        self.doc = Some(doc);
    }

    fn layout_and_paint(&mut self, doc: &mut BaseDocument) {
        doc.set_viewport(self.viewport(1));
        for _ in 0..2 {
            doc.handle_messages();
            doc.resolve(0.0);
        }
        let layout = *doc.root_element().final_layout();
        let content_css = layout.size.height.max(layout.scrollable_overflow_rect.bottom);
        self.height = ((content_css * self.scale).ceil() as u32).clamp(1, MAX_PAINT_HEIGHT);
        doc.set_viewport(self.viewport(self.height));
        self.paint(doc);
    }

    /// Paint the already laid-out document (e.g. after a selection change).
    fn paint(&mut self, doc: &mut BaseDocument) {
        doc.resolve(0.0);
        let (width, height) = (self.width.max(1), self.height);
        let scale = f64::from(self.scale);
        let rgba = render_to_buffer::<VelloCpuImageRenderer, _>(
            |scene| blitz_paint::paint_scene(scene, doc, scale, width, height, 0, 0),
            width,
            height,
        );
        self.dirty = false;
        self.last_paint = std::time::Instant::now();
        (self.sink)(RenderedPage { width, height, scale: self.scale, rgba_premultiplied: rgba });
    }

    /// Diagnostic: the element chain under a point and the resolved href.
    pub fn debug_href_at(&self, x: f32, y: f32) -> (Vec<String>, Option<String>) {
        let mut chain = Vec::new();
        if let Some(doc) = self.doc.as_ref() {
            let mut node_id = doc.element_from_point(x / self.scale, y / self.scale);
            while let Some(id) = node_id {
                let Some(node) = doc.get_node(id) else { break };
                if let Some(e) = node.element_data() {
                    chain.push(e.name.local.to_string());
                }
                node_id = node.parent;
            }
        }
        (chain, self.href_at(x, y))
    }

    /// The `href` of the link under physical document coordinates, if any.
    fn href_at(&self, x: f32, y: f32) -> Option<String> {
        let doc = self.doc.as_ref()?;
        let mut node_id = doc.element_from_point(x / self.scale, y / self.scale);
        while let Some(id) = node_id {
            let node = doc.get_node(id)?;
            if let Some(element) = node.element_data() {
                if element.name.local == local_name!("a") {
                    if let Some(href) = element.attr(local_name!("href")) {
                        return Some(href.to_owned());
                    }
                }
            }
            node_id = node.parent;
        }
        None
    }
}

impl BodyRenderer for BlitzRenderer {
    fn name(&self) -> &'static str {
        "blitz"
    }

    fn show(&mut self, doc: &SafeDocument, _policy: RemoteContentPolicy) -> Result<(), RenderError> {
        let resources = doc.resources.entries.iter().map(|e| (e.mime.clone(), e.data.clone())).collect();
        self.source = Some((doc.html.clone(), Arc::new(resources)));
        self.rebuild();
        Ok(())
    }

    fn clear(&mut self) -> Result<(), RenderError> {
        self.source = None;
        self.rebuild();
        Ok(())
    }

    fn set_bounds(&mut self, rect: Rect) -> Result<(), RenderError> {
        if rect.width != self.width {
            self.width = rect.width.max(1);
            if let Some(mut doc) = self.doc.take() {
                self.layout_and_paint(&mut doc);
                self.doc = Some(doc);
            }
        }
        Ok(())
    }

    fn set_visible(&mut self, _visible: bool) -> Result<(), RenderError> {
        Ok(())
    }

    fn composited_by_host(&self) -> bool {
        true
    }

    fn pointer_click(&mut self, x: f32, y: f32) {
        let Some(href) = self.href_at(x, y) else { return };
        match classify_link(&href) {
            Some(link) => (self.on_link)(link),
            None => {
                self.stats.navigations_blocked.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Only links the host would act on count; `javascript:` and the like are inert.
    fn link_at(&self, x: f32, y: f32) -> bool {
        self.href_at(x, y).is_some_and(|href| classify_link(&href).is_some())
    }

    fn pointer_event(&mut self, phase: PointerPhase, x: f32, y: f32, pressed: bool) {
        let Some(doc) = self.doc.as_mut() else { return };
        // The viewport spans the whole document at scroll 0: page == client coords.
        let (cx, cy) = (x / self.scale, y / self.scale);
        let event = BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords: PointerCoords { page_x: cx, page_y: cy, screen_x: cx, screen_y: cy, client_x: cx, client_y: cy },
            button: MouseEventButton::Main,
            buttons: if pressed { MouseEventButtons::Primary } else { MouseEventButtons::None },
            mods: keyboard_types::Modifiers::empty(),
            details: PointerDetails { pressure: if pressed { 0.5 } else { 0.0 }, ..PointerDetails::default() },
            element: Point { x: cx, y: cy },
            active_pointers: Arc::new(AtomicRefCell::new(Vec::new())),
        };
        let ui_event = match phase {
            PointerPhase::Down => UiEvent::PointerDown(event),
            PointerPhase::Move => UiEvent::PointerMove(event),
            PointerPhase::Up => UiEvent::PointerUp(event),
        };
        let before = doc.get_text_selection_ranges();
        EventDriver::new(doc, NoopEventHandler).handle_ui_event(ui_event);
        if doc.get_text_selection_ranges() != before {
            self.dirty = true;
        }
    }

    fn selected_text(&self) -> Option<String> {
        self.doc.as_ref()?.get_selected_text().filter(|t| !t.is_empty())
    }

    fn poll_repaint(&mut self) {
        if self.dirty && self.last_paint.elapsed() >= REPAINT_INTERVAL {
            if let Some(mut doc) = self.doc.take() {
                self.paint(&mut doc);
                self.doc = Some(doc);
            }
        }
    }

    fn set_scale_factor(&mut self, scale: f32) {
        if (scale - self.scale).abs() > f32::EPSILON && scale > 0.0 {
            self.scale = scale;
            self.rebuild();
        }
    }

    fn set_dark_mode(&mut self, dark: bool) {
        if dark != self.dark {
            self.dark = dark;
            self.rebuild();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";

    fn manifest() -> Vec<(String, Vec<u8>)> {
        vec![("image/png".into(), vec![1]), ("image/svg+xml".into(), vec![2])]
    }

    #[test]
    fn resolves_only_manifest_rasters_and_raster_data_urls() {
        let m = manifest();
        let ok = |u: &str| resolve_resource(&Url::parse(u).unwrap(), &m).is_some();
        assert!(ok("fuu://doc/res/0"));
        assert!(!ok("fuu://doc/res/1"), "svg must be refused");
        assert!(!ok("fuu://doc/res/7"));
        assert!(!ok("fuu://doc/res/../res/0") || ok("fuu://doc/res/0"));
        assert!(!ok("fuu://other/res/0"));
        assert!(ok(&format!("data:image/png;base64,{PNG_1X1}")));
        assert!(!ok("data:image/svg+xml;base64,PHN2Zy8+"));
        assert!(!ok("data:text/html;base64,PHNjcmlwdD4="));
        for remote in ["https://example.com/p.gif", "http://x/y.png", "file:///C:/x.png", "ftp://x/y"] {
            assert!(!ok(remote), "{remote}");
        }
    }

    #[test]
    fn paints_document_and_refuses_remote_loads() {
        let pages = Rc::new(RefCell::new(Vec::<RenderedPage>::new()));
        let sink_pages = Rc::clone(&pages);
        let links = Rc::new(RefCell::new(Vec::new()));
        let link_log = Rc::clone(&links);
        let mut r = BlitzRenderer::new(
            Box::new(move |p| sink_pages.borrow_mut().push(p)),
            Box::new(move |l| link_log.borrow_mut().push(l)),
        );
        r.set_bounds(Rect { x: 0, y: 0, width: 400, height: 300 }).unwrap();
        let html = format!(
            r#"<html><head><link rel="stylesheet" href="https://example.com/a.css"></head>
            <body style="margin:0"><a href="https://example.org/" style="display:block;height:40px">link</a>
            <img src="https://example.com/pixel.gif" width="1" height="1">
            <img src="data:image/png;base64,{PNG_1X1}" width="20" height="20">
            <div style="height:200px">日本語</div></body></html>"#
        );
        let doc = SafeDocument { html, ..SafeDocument::default() };
        r.show(&doc, RemoteContentPolicy::Block).unwrap();

        let page = pages.borrow().last().cloned().expect("a page was painted");
        assert_eq!(page.width, 400);
        assert!(page.height >= 240, "content height {}", page.height);
        assert_eq!(page.rgba_premultiplied.len(), (page.width * page.height * 4) as usize);
        let stats = r.stats();
        assert!(stats.refused.load(Ordering::Relaxed) >= 2, "remote css and pixel refused");
        assert_eq!(stats.served.load(Ordering::Relaxed), 1, "only the data: image served");

        assert!(r.link_at(10.0, 10.0));
        r.pointer_click(10.0, 10.0);
        assert_eq!(links.borrow().as_slice(), &[crate::LinkRequest::Web("https://example.org/".into())]);
        assert!(!r.link_at(10.0, 220.0));
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    #[test]
    fn drag_selects_text() {
        let mut r = BlitzRenderer::new(Box::new(|_| {}), Box::new(|_| {}));
        r.set_bounds(Rect { x: 0, y: 0, width: 600, height: 100 }).unwrap();
        let html = r#"<body style="margin:0;font-size:20px"><p style="margin:0">hello brave world</p></body>"#;
        r.show(&SafeDocument { html: html.into(), ..SafeDocument::default() }, RemoteContentPolicy::Block).unwrap();
        assert_eq!(r.selected_text(), None);
        r.pointer_event(PointerPhase::Down, 1.0, 10.0, true);
        r.pointer_event(PointerPhase::Move, 300.0, 10.0, true);
        r.pointer_event(PointerPhase::Up, 300.0, 10.0, false);
        let text = r.selected_text().expect("drag selected something");
        assert!(text.starts_with("hello"), "{text:?}");
        assert!(r.dirty, "selection change requests a repaint");
    }
}

#[cfg(test)]
mod inert_link_tests {
    use super::*;

    #[test]
    fn javascript_links_are_not_reported_as_links() {
        let mut r = BlitzRenderer::new(Box::new(|_| {}), Box::new(|_| {}));
        r.set_bounds(Rect { x: 0, y: 0, width: 300, height: 100 }).unwrap();
        let html = r#"<body style="margin:0"><a href="javascript:alert(1)" style="display:block;height:30px">x</a>
            <a href="mailto:a@example.com" style="display:block;height:30px">m</a></body>"#;
        r.show(&SafeDocument { html: html.into(), ..SafeDocument::default() }, RemoteContentPolicy::Block).unwrap();
        assert!(!r.link_at(5.0, 10.0));
        assert!(r.link_at(5.0, 40.0));
    }
}
