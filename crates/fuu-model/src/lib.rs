//! Engine-neutral types that cross Fuu's parser / sanitiser / renderer boundaries.
//!
//! Rules (see `ARCHITECTURE.md`):
//! - No third-party crate type ever appears in this crate's public API, so that the
//!   parsing engine and the rendering engine can each be swapped independently.
//! - A renderer only ever receives a [`SafeDocument`]; it never sees raw MIME or a
//!   [`ParsedMessage`].

/// Content-Security-Policy sent as a response header with every document.
/// `'self'` is the renderer's private document origin; nothing else is reachable.
pub const DOCUMENT_CSP: &str = "default-src 'none'; img-src 'self' data:; \
style-src 'unsafe-inline'; font-src 'none'; script-src 'none'; connect-src 'none'; \
frame-src 'none'; object-src 'none'; media-src 'none'; form-action 'none'; base-uri 'none'";

/// A mailbox address as shown to the user.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Address {
    /// Display name with bidirectional control characters already removed.
    pub name: Option<String>,
    pub email: String,
}

/// Header fields Fuu cares about. All strings are UTF-8 normalised.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Headers {
    pub message_id: Option<String>,
    pub in_reply_to: Vec<String>,
    pub references: Vec<String>,
    pub subject: String,
    pub from: Vec<Address>,
    pub reply_to: Vec<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    /// Seconds since the Unix epoch, if the Date header was parseable.
    pub date: Option<i64>,
}

/// A displayable body alternative, already decoded to UTF-8.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyPart {
    Text(String),
    Html(String),
}

/// A resource referenced from the HTML body by `cid:`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineResource {
    /// Content-ID without the surrounding angle brackets.
    pub content_id: String,
    pub mime: String,
    pub data: Vec<u8>,
}

/// A user-visible attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// File name after sanitisation (no path separators or control characters).
    pub file_name: String,
    pub mime: String,
    pub size: u64,
    /// Index into the parser's part list, used to fetch the bytes on demand.
    pub part_index: usize,
}

/// Output of a [`MessageParser`](../fuu_parser/trait.MessageParser.html).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedMessage {
    pub headers: Headers,
    pub body: Vec<BodyPart>,
    pub attachments: Vec<Attachment>,
    pub inline: Vec<InlineResource>,
}

/// One entry of a [`ResourceManifest`]. Renderers serve these by index only; the
/// original `cid:` string is never used as a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    pub mime: String,
    pub data: Vec<u8>,
}

/// Resources a [`SafeDocument`] may load. Index `n` is addressed as
/// `<document origin>/res/<n>` by the renderer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResourceManifest {
    pub entries: Vec<ManifestEntry>,
}

/// Findings the sanitiser reports to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SafetyFlags {
    /// The original HTML referenced remote images, which were blocked.
    pub remote_images: bool,
    /// At least one link's visible text named a different domain, or used IDN.
    pub suspicious_links: bool,
    /// The message exceeded parse limits and was not rendered in full.
    pub truncated: bool,
}

/// Sanitised, self-contained document: the only thing a renderer receives.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SafeDocument {
    /// Complete HTML document. Resource URLs are relative (`res/<n>`).
    pub html: String,
    /// Plain-text rendition for non-HTML renderers and reply quoting.
    pub text: String,
    pub resources: ResourceManifest,
    pub flags: SafetyFlags,
    /// Remote images the user may allow for this message. Each `<img>` points at
    /// `remote/<index>` (never loadable by the renderer) until Fuu fetches the
    /// image and rewrites it to a manifest entry. Tracking-pixel candidates are
    /// not listed.
    pub remote_images: Vec<String>,
}

/// Whether remote content may be fetched (by Fuu, never by the renderer itself).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RemoteContentPolicy {
    #[default]
    Block,
    /// Allowed for the current message only. Tracking-pixel candidates stay blocked.
    AllowForMessage,
}

/// A rectangle in physical pixels relative to the host window's client area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
