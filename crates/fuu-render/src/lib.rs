//! Rendering engine boundary.
//!
//! A [`BodyRenderer`] receives only a [`SafeDocument`] (already sanitised by
//! `fuu-sanitiser`). It never sees raw MIME, a `ParsedMessage`, or the network.
//! Implementations:
//! - [`blitz::BlitzRenderer`]: Blitz HTML/CSS engine, no JavaScript (default).
//! - [`text::TextRenderer`]: plain-text fallback.

pub use fuu_model::{Rect, RemoteContentPolicy, SafeDocument};

#[cfg(feature = "blitz")]
pub mod blitz;
pub mod text;

use std::sync::atomic::AtomicUsize;

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

/// Request counters, for diagnostics and the renderer test-suite.
#[derive(Debug, Default)]
pub struct RenderStats {
    pub served: AtomicUsize,
    pub refused: AtomicUsize,
    pub navigations_blocked: AtomicUsize,
}

/// Mouse cursor an engine asks the host to show. Fuu's own type, so no engine's
/// cursor type crosses the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cursor {
    #[default]
    Default,
    Text,
    Pointer,
    Help,
    NotAllowed,
    Crosshair,
    Move,
    Wait,
    Progress,
    Grab,
    Grabbing,
}

impl Cursor {
    /// Stable index used by the Slint side.
    pub fn index(self) -> i32 {
        self as i32
    }
}

/// Phase of a host-delivered pointer event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerPhase {
    Down,
    Move,
    Up,
}

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
    /// Whether the engine draws through the host (an image the host composites,
    /// so host overlays can sit above it) rather than a native child surface.
    fn composited_by_host(&self) -> bool {
        false
    }
    /// Host-delivered click at physical pixel coordinates within the rendered
    /// document. Engines with their own native input ignore it.
    fn pointer_click(&mut self, _x: f32, _y: f32) {}
    /// Pointer hovering (no button) at physical document coordinates; returns the
    /// cursor to show there.
    fn hover(&mut self, _x: f32, _y: f32) -> Cursor {
        Cursor::Default
    }
    /// Select the whole document.
    fn select_all(&mut self) {}
    /// Drop the current selection.
    fn clear_selection(&mut self) {}
    /// Host-delivered primary-button pointer event at physical document coordinates
    /// (drives text selection in host-composited engines).
    fn pointer_event(&mut self, _phase: PointerPhase, _x: f32, _y: f32, _pressed: bool) {}
    /// Currently selected text, if the engine tracks a selection itself.
    fn selected_text(&self) -> Option<String> {
        None
    }
    /// Called by the host on every frame tick; engines repaint deferred changes here.
    fn poll_repaint(&mut self) {}
    /// Device pixel ratio of the host surface.
    fn set_scale_factor(&mut self, _scale: f32) {}
    /// Light or dark colour scheme for documents that support both.
    fn set_dark_mode(&mut self, _dark: bool) {}
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
