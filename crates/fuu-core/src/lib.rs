//! Mail engine (accounts, IMAP/SMTP, OAuth, cache, compose). UI independent.

pub mod account;
pub mod cache;
pub mod compose;
pub mod config;
pub mod engine;
pub mod imap;
pub mod remote;
pub mod secrets;
pub mod smtp;
pub mod tls;

pub use fuu_parser::{MessageParser, ParseLimits};
pub use fuu_sanitiser::DOCUMENT_CSP;
