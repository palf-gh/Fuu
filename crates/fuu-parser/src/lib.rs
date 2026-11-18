//! Parsing engine boundary. Implementations turn raw RFC 5322 bytes into the
//! engine-neutral [`ParsedMessage`]. The default engine is
//! [`mailparser::MailParserEngine`].

pub mod headers;
pub mod mailparser;

pub use fuu_model::ParsedMessage;

/// Hard limits applied before anything is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseLimits {
    pub max_message_bytes: usize,
    pub max_mime_depth: usize,
    pub max_parts: usize,
    pub max_decoded_bytes: usize,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            max_message_bytes: 25 * 1024 * 1024,
            max_mime_depth: 16,
            max_parts: 512,
            max_decoded_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    TooLarge,
    TooDeep,
    TooManyParts,
    Malformed(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => f.write_str("message exceeds the size limit"),
            Self::TooDeep => f.write_str("MIME structure is nested too deeply"),
            Self::TooManyParts => f.write_str("message has too many MIME parts"),
            Self::Malformed(why) => write!(f, "malformed message: {why}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// A swappable parsing engine.
pub trait MessageParser: Send + Sync {
    fn parse(&self, raw: &[u8], limits: &ParseLimits) -> Result<ParsedMessage, ParseError>;
}
