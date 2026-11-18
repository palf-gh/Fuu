//! Hardened wry child webview.
//!
//! Defence layers implemented here (see `SECURITY.md`, layer 3):
//! - JavaScript disabled at the engine level (`with_javascript_disabled`).
//! - Every document is served from a private custom protocol with a strict CSP
//!   response header; the webview never talks to the network.
//! - Resources are addressed by manifest index under a per-document token; a stale
//!   token or unknown path gets 404, and only raster image types are served.
//! - All navigations away from the document are cancelled; `http(s)`/`mailto`
//!   activations are handed to the host, everything else is dropped.
//! - No IPC handler, no devtools, incognito, no drag and drop, no autofill,
//!   no default context menu or browser accelerator keys.

use std::borrow::Cow;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use raw_window_handle::HasWindowHandle;
use wry::dpi::{PhysicalPosition, PhysicalSize};
use wry::http::{header, Request, Response, StatusCode};
use wry::{NewWindowResponse, WebView, WebViewBuilder};

use crate::{classify_link, BodyRenderer, RenderStats, LinkHandler, Rect, RemoteContentPolicy, RenderError, SafeDocument};
use fuu_model::DOCUMENT_CSP;

const SCHEME: &str = "fuu";

/// Origin under which documents are served. wry maps custom protocols to
/// `https://<scheme>.localhost` on Windows (with the https scheme enabled) and to
/// `<scheme>://localhost` elsewhere.
pub fn document_origin() -> &'static str {
    if cfg!(windows) {
        "https://fuu.localhost"
    } else {
        "fuu://localhost"
    }
}

/// MIME types a document may load as resources. Anything else is refused,
/// including SVG (scriptable) and HTML.
const SERVABLE_RESOURCE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

#[derive(Default)]
struct Served {
    token: String,
    html: String,
    resources: Vec<(String, Vec<u8>)>,
}

pub struct WebViewRenderer {
    webview: WebView,
    state: Arc<Mutex<Served>>,
    stats: Arc<RenderStats>,
}

fn new_token() -> String {
    let mut h = RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default(),
    );
    format!("{:016x}", h.finish())
}

fn to_wry_rect(rect: Rect) -> wry::Rect {
    wry::Rect {
        position: PhysicalPosition::new(rect.x, rect.y).into(),
        size: PhysicalSize::new(rect.width.max(1), rect.height.max(1)).into(),
    }
}

fn respond(status: StatusCode, content_type: &str, body: Vec<u8>) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_SECURITY_POLICY, DOCUMENT_CSP)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::REFERRER_POLICY, "no-referrer")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Cow::Owned(body))
        .unwrap_or_else(|_| Response::new(Cow::Borrowed(&[][..])))
}

fn not_found() -> Response<Cow<'static, [u8]>> {
    respond(StatusCode::NOT_FOUND, "text/plain", Vec::new())
}

/// Resolve a custom-protocol request against the current document only.
fn serve(state: &Mutex<Served>, stats: &RenderStats, request: &Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let refuse = || {
        stats.refused.fetch_add(1, Ordering::Relaxed);
        not_found()
    };
    if request.method() != wry::http::Method::GET {
        return refuse();
    }
    let Ok(state) = state.lock() else { return refuse() };
    let path = request.uri().path();
    let mut segments = path.trim_start_matches('/').split('/');
    let (Some(token), Some(first)) = (segments.next(), segments.next()) else { return refuse() };
    if token.is_empty() || token != state.token {
        return refuse();
    }
    let response = match (first, segments.next(), segments.next()) {
        ("index.html", None, _) => {
            respond(StatusCode::OK, "text/html; charset=utf-8", state.html.clone().into_bytes())
        }
        ("res", Some(index), None) => {
            let entry = index.parse::<usize>().ok().and_then(|i| state.resources.get(i));
            match entry {
                Some((mime, data)) if SERVABLE_RESOURCE_TYPES.contains(&mime.as_str()) => {
                    respond(StatusCode::OK, mime, data.clone())
                }
                _ => return refuse(),
            }
        }
        _ => return refuse(),
    };
    stats.served.fetch_add(1, Ordering::Relaxed);
    response
}

impl WebViewRenderer {
    /// Create the webview as a child of `parent`, which must already be realised.
    pub fn new<W: HasWindowHandle>(parent: &W, bounds: Rect, on_link: LinkHandler) -> Result<Self, RenderError> {
        let state = Arc::new(Mutex::new(Served { token: new_token(), ..Served::default() }));
        let stats = Arc::new(RenderStats::default());
        let on_link = std::rc::Rc::new(on_link);
        let initial_url = format!("{}/{}/index.html", document_origin(), state.lock().map(|s| s.token.clone()).unwrap_or_default());

        let protocol_state = Arc::clone(&state);
        let protocol_stats = Arc::clone(&stats);
        let nav_stats = Arc::clone(&stats);
        let nav_link = std::rc::Rc::clone(&on_link);
        let window_link = std::rc::Rc::clone(&on_link);
        let window_stats = Arc::clone(&stats);
        let own_prefix = format!("{}/", document_origin());

        let builder = WebViewBuilder::new()
            .with_javascript_disabled()
            .with_devtools(false)
            .with_incognito(true)
            .with_clipboard(false)
            .with_autoplay(false)
            .with_hotkeys_zoom(false)
            .with_back_forward_navigation_gestures(false)
            .with_general_autofill_enabled(false)
            .with_drag_drop_handler(|_| true)
            .with_bounds(to_wry_rect(bounds))
            .with_custom_protocol(SCHEME.to_owned(), move |_id, request| {
                serve(&protocol_state, &protocol_stats, &request)
            })
            .with_navigation_handler(move |url: String| {
                if url.starts_with(&own_prefix) {
                    return true;
                }
                nav_stats.navigations_blocked.fetch_add(1, Ordering::Relaxed);
                if let Some(link) = classify_link(&url) {
                    (nav_link)(link);
                }
                false
            })
            .with_new_window_req_handler(move |url: String, _features| {
                window_stats.navigations_blocked.fetch_add(1, Ordering::Relaxed);
                if let Some(link) = classify_link(&url) {
                    (window_link)(link);
                }
                NewWindowResponse::Deny
            })
            .with_url(initial_url);

        #[cfg(windows)]
        let builder = {
            use wry::WebViewBuilderExtWindows;
            builder
                .with_https_scheme(true)
                .with_browser_accelerator_keys(false)
                .with_default_context_menus(false)
        };

        let webview = builder
            .build_as_child(parent)
            .map_err(|e| RenderError::Unavailable(e.to_string()))?;
        Ok(Self { webview, state, stats })
    }

    pub fn stats(&self) -> Arc<RenderStats> {
        Arc::clone(&self.stats)
    }

    fn load(&mut self, html: String, resources: Vec<(String, Vec<u8>)>) -> Result<(), RenderError> {
        let token = new_token();
        {
            let mut state = self.state.lock().map_err(|e| RenderError::Backend(e.to_string()))?;
            *state = Served { token: token.clone(), html, resources };
        }
        self.webview
            .load_url(&format!("{}/{}/index.html", document_origin(), token))
            .map_err(|e| RenderError::Backend(e.to_string()))
    }
}

impl BodyRenderer for WebViewRenderer {
    fn name(&self) -> &'static str {
        "webview"
    }

    /// Remote content is never fetched by the webview. With
    /// [`RemoteContentPolicy::AllowForMessage`] the host fetches allowed images
    /// itself and adds them to the manifest (M2+), so `policy` is unused here.
    fn show(&mut self, doc: &SafeDocument, _policy: RemoteContentPolicy) -> Result<(), RenderError> {
        let resources = doc
            .resources
            .entries
            .iter()
            .map(|e| (e.mime.clone(), e.data.clone()))
            .collect();
        self.load(doc.html.clone(), resources)
    }

    fn clear(&mut self) -> Result<(), RenderError> {
        self.load(String::new(), Vec::new())
    }

    fn set_bounds(&mut self, rect: Rect) -> Result<(), RenderError> {
        self.webview
            .set_bounds(to_wry_rect(rect))
            .map_err(|e| RenderError::Backend(e.to_string()))
    }

    fn set_visible(&mut self, visible: bool) -> Result<(), RenderError> {
        self.webview
            .set_visible(visible)
            .map_err(|e| RenderError::Backend(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with(token: &str) -> Mutex<Served> {
        Mutex::new(Served {
            token: token.into(),
            html: "<p>hi</p>".into(),
            resources: vec![
                ("image/png".into(), vec![1, 2, 3]),
                ("image/svg+xml".into(), b"<svg/>".to_vec()),
                ("text/html".into(), b"<script>".to_vec()),
            ],
        })
    }

    fn get(path: &str) -> Request<Vec<u8>> {
        Request::builder().uri(format!("{}{}", document_origin(), path)).body(Vec::new()).unwrap()
    }

    #[test]
    fn serves_current_document_with_csp() {
        let stats = RenderStats::default();
        let r = serve(&state_with("abc"), &stats, &get("/abc/index.html"));
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(r.headers()[header::CONTENT_SECURITY_POLICY], DOCUMENT_CSP);
        assert_eq!(r.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert_eq!(r.body().as_ref(), b"<p>hi</p>");
    }

    #[test]
    fn serves_raster_resources_only() {
        let stats = RenderStats::default();
        let s = state_with("abc");
        assert_eq!(serve(&s, &stats, &get("/abc/res/0")).status(), StatusCode::OK);
        assert_eq!(serve(&s, &stats, &get("/abc/res/1")).status(), StatusCode::NOT_FOUND);
        assert_eq!(serve(&s, &stats, &get("/abc/res/2")).status(), StatusCode::NOT_FOUND);
        assert_eq!(serve(&s, &stats, &get("/abc/res/9")).status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn refuses_stale_tokens_traversal_and_other_methods() {
        let stats = RenderStats::default();
        let s = state_with("abc");
        for path in ["/old/index.html", "/abc/../abc/index.html", "/abc/res/../index.html", "/abc/res/0/x", "//index.html", "/"] {
            assert_eq!(serve(&s, &stats, &get(path)).status(), StatusCode::NOT_FOUND, "{path}");
        }
        let post = Request::builder()
            .method("POST")
            .uri(format!("{}/abc/index.html", document_origin()))
            .body(Vec::new())
            .unwrap();
        assert_eq!(serve(&s, &stats, &post).status(), StatusCode::NOT_FOUND);
        assert_eq!(stats.served.load(Ordering::Relaxed), 0);
        assert_eq!(stats.refused.load(Ordering::Relaxed), 7);
    }
}
