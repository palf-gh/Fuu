//! Attachment previews. Images, PDF and Office files become neutral HTML with
//! `cid:` image references plus inline raster resources, packed as a
//! [`ParsedMessage`] so the app can run the sanitiser on it exactly as on a
//! mail body. This crate knows nothing about UI or rendering, and its output is
//! never shown without sanitising.
//!
//! Every converter treats its input as hostile: sizes, pixel counts, page and
//! row counts and zip entries are capped, external references (linked images,
//! remote templates) are dropped, and nothing is written to disk.

mod docx;
mod html;
mod image;
mod ooxml;
mod pdf;
mod pptx;
mod xlsx;

use fuu_model::{BodyPart, Headers, InlineResource, ParsedMessage};

/// Largest attachment previewed at all.
pub const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;

/// What a file previews as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Image,
    Pdf,
    Word,
    Spreadsheet,
    Slides,
}

impl Kind {
    /// Japanese label for the reader header.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Image => "画像",
            Kind::Pdf => "PDF",
            Kind::Word => "Word 文書",
            Kind::Spreadsheet => "表計算",
            Kind::Slides => "プレゼンテーション",
        }
    }
}

fn extension(file_name: &str) -> String {
    file_name.rsplit_once('.').map(|(_, e)| e.trim().to_ascii_lowercase()).unwrap_or_default()
}

/// The preview kind of an attachment, from its extension (preferred: senders
/// often label everything `application/octet-stream`) or its MIME type.
pub fn kind_of(file_name: &str, mime: &str) -> Option<Kind> {
    let by_ext = match extension(file_name).as_str() {
        "png" | "jpg" | "jpeg" | "jpe" | "gif" | "webp" | "bmp" | "tif" | "tiff" => Some(Kind::Image),
        "pdf" => Some(Kind::Pdf),
        "docx" | "docm" => Some(Kind::Word),
        // xls / xlsb / ods: their sheet sizes cannot be checked before loading.
        "xlsx" | "xlsm" => Some(Kind::Spreadsheet),
        "pptx" | "pptm" => Some(Kind::Slides),
        _ => None,
    };
    by_ext.or_else(|| match mime.to_ascii_lowercase().as_str() {
        "image/png" | "image/jpeg" | "image/jpg" | "image/gif" | "image/webp" | "image/bmp" | "image/tiff" => Some(Kind::Image),
        "application/pdf" => Some(Kind::Pdf),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => Some(Kind::Word),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => Some(Kind::Spreadsheet),
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => Some(Kind::Slides),
        _ => None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewError {
    Unsupported,
    TooLarge,
    /// The file could not be read (damaged, encrypted, or not what it claims).
    Unreadable(String),
}

impl std::fmt::Display for PreviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => f.write_str("この形式はプレビューできません"),
            Self::TooLarge => f.write_str("ファイルが大きすぎるためプレビューできません"),
            Self::Unreadable(why) => write!(f, "ファイルを読み取れませんでした（{why}）"),
        }
    }
}

impl std::error::Error for PreviewError {}

/// Neutral HTML body and its images, before packing.
#[derive(Debug, Default)]
pub(crate) struct Output {
    pub html: String,
    pub images: Vec<InlineResource>,
}

impl Output {
    /// Add an image resource and return its `cid:` URL.
    pub fn add_image(&mut self, mime: &str, data: Vec<u8>) -> String {
        let id = format!("preview-{}@fuu.invalid", self.images.len());
        self.images.push(InlineResource { content_id: id.clone(), mime: mime.to_owned(), data });
        format!("cid:{id}")
    }
}

/// Convert an attachment to a message-shaped document for the sanitiser.
/// Runs synchronously and may take a while (PDF): call it off the UI thread.
pub fn preview(file_name: &str, mime: &str, data: &[u8]) -> Result<ParsedMessage, PreviewError> {
    let kind = kind_of(file_name, mime).ok_or(PreviewError::Unsupported)?;
    if data.len() > MAX_INPUT_BYTES {
        return Err(PreviewError::TooLarge);
    }
    let out = match kind {
        Kind::Image => image::preview(data)?,
        Kind::Pdf => pdf::preview(data)?,
        Kind::Word => docx::preview(data)?,
        Kind::Spreadsheet => xlsx::preview(data)?,
        Kind::Slides => pptx::preview(data)?,
    };
    let html = format!(
        "<html><head><style>{}</style></head><body>{}</body></html>",
        html::STYLE,
        out.html
    );
    Ok(ParsedMessage {
        headers: Headers { subject: file_name.to_owned(), ..Headers::default() },
        body: vec![BodyPart::Html(html)],
        attachments: Vec::new(),
        inline: out.images,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_extension_then_mime() {
        assert_eq!(kind_of("写真.JPG", "application/octet-stream"), Some(Kind::Image));
        assert_eq!(kind_of("report.pdf", ""), Some(Kind::Pdf));
        assert_eq!(kind_of("見積書.xlsx", ""), Some(Kind::Spreadsheet));
        assert_eq!(kind_of("old.xls", "application/vnd.ms-excel"), None, "unbounded sheet sizes");
        assert_eq!(kind_of("deck.pptx", ""), Some(Kind::Slides));
        assert_eq!(kind_of("letter.docx", ""), Some(Kind::Word));
        assert_eq!(kind_of("noext", "application/pdf"), Some(Kind::Pdf));
        assert_eq!(kind_of("old.doc", "application/msword"), None);
        assert_eq!(kind_of("vector.svg", "image/svg+xml"), None, "SVG is scriptable");
        assert_eq!(kind_of("setup.exe", "application/octet-stream"), None);
    }

    #[test]
    fn unsupported_and_oversized_inputs_are_refused() {
        assert_eq!(preview("a.exe", "", b"MZ"), Err(PreviewError::Unsupported));
        let big = vec![0u8; MAX_INPUT_BYTES + 1];
        assert_eq!(preview("a.pdf", "", &big), Err(PreviewError::TooLarge));
    }
}
