//! Rendering engine boundary.
//!
//! A [`BodyRenderer`] receives only a [`SafeDocument`] (already sanitised by
//! `fuu-sanitiser`). It never sees raw MIME, a `ParsedMessage`, or the network.
//! Implementations:
//! - [`webview::WebViewRenderer`]: hardened wry child webview (default).
//! - [`text::TextRenderer`]: plain-text fallback for hosts without a webview.

pub use fuu_model::{Rect, RemoteContentPolicy, SafeDocument};

pub mod text;
pub mod webview;

/// What the user asked to do with a link inside a rendered message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkRequest {
    /// `http`/`https`: the host must confirm, then open it in the OS browser.
    Web(String),
    /// `mailto:`: the host opens its own composer.
    Mailto(String),
}

/// Callback through which a renderer reports link activations to the host.
pub type LinkHandler = Box<dyn Fn(LinkRequest) + 'static>;

#[derive(Debug)]
pub enum RenderError {
    Unavailable(String),
    Backend(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(why) => write!(f, "renderer unavailable: {why}"),
            Self::Backend(why) => write!(f, "renderer error: {why}"),
        }
    }
}

impl std::error::Error for RenderError {}

/// A swappable rendering engine.
pub trait BodyRenderer {
    /// Short engine identifier for diagnostics.
    fn name(&self) -> &'static str;
    /// Replace the displayed document. Any previous document and its resources are
    /// discarded and become unreachable.
    fn show(&mut self, doc: &SafeDocument, policy: RemoteContentPolicy) -> Result<(), RenderError>;
    /// Discard the current document.
    fn clear(&mut self) -> Result<(), RenderError>;
    /// Position in physical pixels relative to the host window's client area.
    fn set_bounds(&mut self, rect: Rect) -> Result<(), RenderError>;
    /// Hide while host popups or dialogs overlap the reading pane.
    fn set_visible(&mut self, visible: bool) -> Result<(), RenderError>;
}

/// Classify a URL the user activated. Anything that is not `http`, `https` or
/// `mailto` is dropped.
pub fn classify_link(url: &str) -> Option<LinkRequest> {
    let trimmed = url.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        Some(LinkRequest::Web(trimmed.to_owned()))
    } else if lower.starts_with("mailto:") {
        Some(LinkRequest::Mailto(trimmed.to_owned()))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_only_allowed_schemes() {
        assert_eq!(
            classify_link("https://example.org/a"),
            Some(LinkRequest::Web("https://example.org/a".into()))
        );
        assert_eq!(classify_link("HTTP://x"), Some(LinkRequest::Web("HTTP://x".into())));
        assert_eq!(classify_link("mailto:a@b"), Some(LinkRequest::Mailto("mailto:a@b".into())));
        for bad in ["javascript:alert(1)", "file:///C:/x", "data:text/html,x", "ftp://x", "vbscript:x", ""] {
            assert_eq!(classify_link(bad), None, "{bad}");
        }
    }
}
