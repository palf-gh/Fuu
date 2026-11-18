//! Mail engine (accounts, IMAP/SMTP, OAuth, cache, compose). UI independent.

pub mod account;
pub mod config;
pub mod engine;
pub mod imap;
pub mod secrets;

pub use fuu_parser::{MessageParser, ParseLimits};
pub use fuu_sanitiser::DOCUMENT_CSP;
