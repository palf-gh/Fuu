//! Rich-text compose editor for Fuu.
//!
//! - [`model`]: the document (text + style runs, line-prefix blocks), edits and
//!   HTML/plain export.
//! - [`history`]: undo/redo.

pub mod history;
pub mod model;
pub mod view;
