//! Undo/redo for the rich editor. Each entry is a full snapshot (mail bodies are
//! small); consecutive single-character typing is grouped into one step.

use crate::model::RichDoc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub doc: RichDoc,
    pub selection: (usize, usize),
}

#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// The last recorded change was ordinary typing that may be grouped.
    typing: bool,
    typing_bytes: usize,
}

const MAX_STEPS: usize = 200;
const GROUP_BYTES: usize = 48;

impl History {
    /// Record the state *before* a change. `typing` marks a small insertion.
    pub fn record(&mut self, before: Snapshot, typing: bool, inserted: &str) {
        let groupable = typing && self.typing && self.typing_bytes < GROUP_BYTES && !inserted.contains(['\n', ' ', '\u{3000}']);
        if !groupable {
            self.undo.push(before);
            if self.undo.len() > MAX_STEPS {
                self.undo.remove(0);
            }
            self.typing_bytes = 0;
        }
        self.typing = typing;
        self.typing_bytes += inserted.len();
        self.redo.clear();
    }

    /// Break typing groups (caret moved, formatting applied).
    pub fn seal(&mut self) {
        self.typing = false;
    }

    pub fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let previous = self.undo.pop()?;
        self.redo.push(current);
        self.typing = false;
        Some(previous)
    }

    pub fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        self.typing = false;
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(text: &str) -> Snapshot {
        Snapshot { doc: RichDoc::from_plain(text), selection: (text.len(), text.len()) }
    }

    #[test]
    fn groups_typing_and_undoes_redoes() {
        let mut h = History::default();
        h.record(snap(""), true, "a");
        h.record(snap("a"), true, "b");
        h.record(snap("ab"), true, "c");
        h.record(snap("abc"), true, " ");
        h.record(snap("abc "), false, "XYZ");
        let s = h.undo(snap("abc XYZ")).unwrap();
        assert_eq!(s.doc.text(), "abc ");
        let s = h.undo(s).unwrap();
        assert_eq!(s.doc.text(), "abc");
        let s = h.undo(s).unwrap();
        assert_eq!(s.doc.text(), "", "typing grouped into one step");
        assert!(h.undo(s.clone()).is_none());
        let s = h.redo(s).unwrap();
        assert_eq!(s.doc.text(), "abc");
    }

    #[test]
    fn new_change_clears_redo() {
        let mut h = History::default();
        h.record(snap(""), false, "x");
        let s = h.undo(snap("x")).unwrap();
        h.record(s, false, "y");
        assert!(h.redo(snap("y")).is_none());
    }
}
