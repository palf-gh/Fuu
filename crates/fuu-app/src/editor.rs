//! Runtime glue between the composer's `RichEditor` (Slint) and `fuu-editor`.
//!
//! The Slint side owns a transparent TextInput (focus, keys, IME, clipboard) whose
//! text mirrors the document's plain text. Edits arrive as whole-text snapshots and
//! are diffed into the rich document; Rust lays the document out, paints it and
//! hands back the page image plus caret and selection geometry.

use std::cell::RefCell;
use std::ops::Range;

use fuu_editor::history::{History, Snapshot};
use fuu_editor::model::{Block, Mark, RichDoc, Size, QUOTE, RULE};
use fuu_editor::view::{EditorView, Palette};
use slint::{ComponentHandle, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};

use crate::{AppWindow, EditorRect};

struct Editor {
    view: EditorView,
    doc: RichDoc,
    history: History,
    anchor: usize,
    cursor: usize,
    preedit: String,
    width: f32,
    /// Word/line range picked by a double/triple click, kept while dragging.
    drag_unit: Option<Range<usize>>,
    sel_serial: i32,
    focus_serial: i32,
    /// (dark, scale) the page was last painted for.
    painted_for: Option<(bool, f32)>,
}

impl Editor {
    fn new() -> Self {
        Editor {
            view: EditorView::new(),
            doc: RichDoc::default(),
            history: History::default(),
            anchor: 0,
            cursor: 0,
            preedit: String::new(),
            width: 600.0,
            drag_unit: None,
            sel_serial: 0,
            focus_serial: 0,
            painted_for: None,
        }
    }

    fn clamp(&self, offset: usize) -> usize {
        let text = self.doc.text();
        let mut o = offset.min(text.len());
        while !text.is_char_boundary(o) {
            o -= 1;
        }
        o
    }

    fn selection(&self) -> Range<usize> {
        let (a, c) = (self.clamp(self.anchor), self.clamp(self.cursor));
        a.min(c)..a.max(c)
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot { doc: self.doc.clone(), selection: (self.anchor, self.cursor) }
    }
}

thread_local! {
    static EDITOR: RefCell<Editor> = RefCell::new(Editor::new());
}

fn with<R>(f: impl FnOnce(&mut Editor) -> R) -> R {
    EDITOR.with(|e| f(&mut e.borrow_mut()))
}

/// Convert plain text with `> ` quoting (as produced for replies) into quote blocks.
#[cfg(test)]
pub fn doc_from_plain(text: &str) -> RichDoc {
    let converted: Vec<String> = text
        .split('\n')
        .map(|line| {
            if let Some(rest) = line.strip_prefix("> ") {
                format!("{QUOTE}{rest}")
            } else if line == ">" {
                QUOTE.to_owned()
            } else {
                line.to_owned()
            }
        })
        .collect();
    RichDoc::from_plain(&converted.join("\n"))
}

/// Reply/forward body: an empty line to type on, the signature, then a rule, the
/// original message's header block (labels in bold) and its text.
pub fn original_doc(signature: &str, header: &str, original: &str) -> RichDoc {
    let mut text = String::from("\n");
    if !signature.trim().is_empty() {
        text.push_str(signature.trim_end());
        text.push('\n');
    }
    text.push('\n');
    let header_start = text.len() + RULE.len() + 1;
    text.push_str(RULE);
    text.push('\n');
    text.push_str(header);
    let header_end = text.len();
    text.push('\n');
    text.push_str(original.trim_end());
    let mut doc = RichDoc::from_plain(&text);
    let mut line_start = header_start;
    for line in text[header_start..header_end].split_inclusive('\n') {
        if let Some(label) = fuu_core::compose::HEADER_LABELS.iter().find(|l| line.starts_with(**l)) {
            doc.toggle_mark(line_start..line_start + label.len(), Mark::Bold);
        }
        line_start += line.len();
    }
    doc
}

/// Show `doc` in the composer, with the caret at the start.
pub fn load(ui: &AppWindow, doc: RichDoc) {
    with(|e| {
        e.doc = doc;
        e.history = History::default();
        e.anchor = 0;
        e.cursor = 0;
        e.preedit.clear();
        e.drag_unit = None;
    });
    ui.set_editor_text(with(|e| e.doc.text().to_owned()).into());
    push_selection(ui);
    refresh(ui);
}

/// The document currently being edited.
pub fn current() -> RichDoc {
    with(|e| e.doc.clone())
}

/// Ask the composer to focus the editor.
pub fn focus(ui: &AppWindow) {
    let serial = with(|e| {
        e.focus_serial += 1;
        e.focus_serial
    });
    ui.set_editor_focus_serial(serial);
}

fn push_selection(ui: &AppWindow) {
    let (serial, a, c) = with(|e| {
        e.sel_serial += 1;
        (e.sel_serial, e.clamp(e.anchor), e.clamp(e.cursor))
    });
    ui.set_editor_sel_anchor(a as i32);
    ui.set_editor_sel_cursor(c as i32);
    ui.set_editor_sel_serial(serial);
}

/// Repaint after a theme or display-scale change (called from the sync timer).
pub fn poll(ui: &AppWindow) {
    if !ui.get_composing() {
        return;
    }
    let current = (ui.get_dark_mode(), ui.window().scale_factor());
    if with(|e| e.painted_for != Some(current)) {
        refresh(ui);
    }
}

/// The editor regained focus: put the document selection back on the TextInput.
pub fn focus_gained(ui: &AppWindow) {
    push_selection(ui);
}

/// Re-lay out and repaint, then update caret, selection and toolbar.
fn refresh(ui: &AppWindow) {
    let scale = ui.window().scale_factor();
    let palette = if ui.get_dark_mode() { Palette::dark() } else { Palette::light() };
    let page = with(|e| {
        let caret = e.clamp(e.cursor);
        let preedit = (!e.preedit.is_empty()).then_some((caret, e.preedit.as_str()));
        let (doc, width) = (&e.doc, e.width);
        e.view.relayout(doc, preedit, width, scale, palette);
        e.painted_for = Some((palette == Palette::dark(), scale));
        e.view.paint()
    });
    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&page.rgba_premultiplied, page.width, page.height);
    ui.set_editor_page(slint::Image::from_rgba8_premultiplied(buffer));
    ui.set_editor_page_height(page.height as f32 / scale);
    update_geometry(ui);
}

/// Caret, selection rectangles and toolbar state (no repaint).
fn update_geometry(ui: &AppWindow) {
    let (caret, rects, style, block) = with(|e| {
        let (a, c) = (e.clamp(e.anchor), e.clamp(e.cursor));
        let caret = e.view.caret(c);
        let rects: Vec<EditorRect> = e
            .view
            .selection_rects(a, c)
            .into_iter()
            .map(|r| EditorRect { x: r.x, y: r.y, w: r.w, h: r.h })
            .collect();
        let range = e.selection();
        let style = if range.is_empty() {
            e.doc.typing.clone().unwrap_or_else(|| e.doc.style_at(range.start))
        } else {
            e.doc.common_style(range.clone())
        };
        let line = e.doc.lines_in(range.start..range.start).into_iter().next().unwrap_or(0..0);
        let (block, _) = RichDoc::block_of(&e.doc.text()[line]);
        (caret, rects, style, block)
    });
    ui.set_editor_caret_x(caret.x);
    ui.set_editor_caret_y(caret.y);
    ui.set_editor_caret_h(caret.h);
    ui.set_editor_selection(ModelRc::new(VecModel::from(rects)));
    ui.set_editor_bold(style.bold);
    ui.set_editor_italic(style.italic);
    ui.set_editor_underline(style.underline);
    ui.set_editor_strike(style.strike);
    ui.set_editor_block(match block {
        Block::Paragraph | Block::Rule => 0,
        Block::Bullet => 1,
        Block::Number => 2,
        Block::Quote => 3,
    });
}

/// The TextInput's text changed (typing, paste, cut, delete, IME commit).
pub fn edited(ui: &AppWindow, text: &str) {
    let changed = with(|e| {
        if e.doc.text() == text {
            return false;
        }
        let before = e.snapshot();
        let old_len = e.doc.len();
        let range = e.doc.apply_text(text);
        let inserted = &text[range.clone()];
        let removed = old_len + inserted.len() - text.len();
        let typing = removed == 0 && inserted.chars().count() <= 2;
        e.history.record(before, typing, inserted);
        // Until the TextInput reports its caret, assume it follows the insertion.
        e.anchor = range.end;
        e.cursor = range.end;
        true
    });
    if changed {
        refresh(ui);
    }
}

/// The TextInput moved its caret or selection by itself (arrows, Ctrl+A, edits).
pub fn caret_moved(ui: &AppWindow, anchor: i32, cursor: i32) {
    let moved = with(|e| {
        let (a, c) = (anchor.max(0) as usize, cursor.max(0) as usize);
        if (a, c) == (e.anchor, e.cursor) {
            return false;
        }
        e.anchor = a;
        e.cursor = c;
        e.history.seal();
        true
    });
    if moved {
        update_geometry(ui);
    }
}

pub fn preedit_changed(ui: &AppWindow, text: &str) {
    let changed = with(|e| {
        if e.preedit == text {
            return false;
        }
        e.preedit = text.to_owned();
        true
    });
    if changed {
        refresh(ui);
    }
}

pub fn resized(ui: &AppWindow, width: f32) {
    let changed = with(|e| {
        if width < 40.0 || (e.width - width).abs() < 0.5 {
            return false;
        }
        e.width = width;
        true
    });
    if changed {
        refresh(ui);
    }
}

/// Up/Down/Home/End/PageUp/PageDown, which need the visual layout.
pub fn navigate(ui: &AppWindow, key: &str, extend: bool, page_height: f32) {
    with(|e| {
        let (a, c) = (e.clamp(e.anchor), e.clamp(e.cursor));
        let (na, nc) = match key {
            "up" => e.view.move_lines(a, c, -1, extend),
            "down" => e.view.move_lines(a, c, 1, extend),
            "home" => e.view.line_start(a, c, extend),
            "end" => e.view.line_end(a, c, extend),
            "pageup" => e.view.move_lines(a, c, -e.view.lines_per_page(page_height), extend),
            "pagedown" => e.view.move_lines(a, c, e.view.lines_per_page(page_height), extend),
            _ => (a, c),
        };
        e.anchor = na;
        e.cursor = nc;
        e.doc.typing = None;
        e.history.seal();
    });
    push_selection(ui);
    update_geometry(ui);
}

/// Return inside a list or quote. `false` lets the TextInput insert a newline.
pub fn enter(ui: &AppWindow) -> bool {
    let handled = with(|e| {
        let range = e.selection();
        if !range.is_empty() || !e.preedit.is_empty() {
            return false;
        }
        let before = e.snapshot();
        match e.doc.enter_in_block(range.start) {
            Some(caret) => {
                e.history.record(before, false, "\n");
                e.anchor = caret;
                e.cursor = caret;
                true
            }
            None => false,
        }
    });
    if handled {
        ui.set_editor_text(with(|e| e.doc.text().to_owned()).into());
        push_selection(ui);
        refresh(ui);
    }
    handled
}

/// Mouse in the page: kind 0 press, 1 drag, 2 release; clicks 1..3.
pub fn pointer(ui: &AppWindow, kind: i32, x: f32, y: f32, clicks: i32, shift: bool) {
    if kind == 2 {
        with(|e| e.drag_unit = None);
        return;
    }
    with(|e| {
        let hit = e.view.hit(x, y);
        if kind == 0 {
            e.doc.typing = None;
            e.history.seal();
            match clicks {
                2 | 3 => {
                    let unit = if clicks == 2 { e.view.word_at(x, y) } else { e.view.line_at(x, y) };
                    e.anchor = unit.start;
                    e.cursor = unit.end;
                    e.drag_unit = Some(unit);
                }
                _ => {
                    if !shift {
                        e.anchor = hit;
                    }
                    e.cursor = hit;
                    e.drag_unit = None;
                }
            }
        } else {
            match e.drag_unit.clone() {
                Some(unit) => {
                    let other = if clicks == 2 { e.view.word_at(x, y) } else { e.view.line_at(x, y) };
                    if other.start < unit.start {
                        e.anchor = unit.end;
                        e.cursor = other.start;
                    } else {
                        e.anchor = unit.start;
                        e.cursor = other.end.max(unit.end);
                    }
                }
                None => e.cursor = hit,
            }
        }
    });
    push_selection(ui);
    update_geometry(ui);
}

/// Toolbar and shortcut commands.
pub fn command(ui: &AppWindow, name: &str) {
    enum Outcome {
        Nothing,
        Styled,
        Text,
    }
    let outcome = with(|e| {
        let range = e.selection();
        let before = e.snapshot();
        let mark = |m: Mark, e: &mut Editor| {
            e.doc.toggle_mark(range.clone(), m);
            if range.is_empty() {
                Outcome::Nothing
            } else {
                e.history.record(before.clone(), false, "");
                Outcome::Styled
            }
        };
        match name {
            "bold" => mark(Mark::Bold, e),
            "italic" => mark(Mark::Italic, e),
            "underline" => mark(Mark::Underline, e),
            "strike" => mark(Mark::Strike, e),
            "undo" | "redo" => {
                let current = e.snapshot();
                let restored = if name == "undo" { e.history.undo(current) } else { e.history.redo(current) };
                match restored {
                    Some(s) => {
                        e.doc = s.doc;
                        (e.anchor, e.cursor) = s.selection;
                        Outcome::Text
                    }
                    None => Outcome::Nothing,
                }
            }
            "unlink" => {
                e.doc.set_link(range.clone(), None);
                e.history.record(before, false, "");
                Outcome::Styled
            }
            "clear" => {
                e.doc.clear_formatting(range.clone());
                e.history.record(before, false, "");
                Outcome::Styled
            }
            "bullet" | "number" | "quote" => {
                let kind = match name {
                    "bullet" => Block::Bullet,
                    "number" => Block::Number,
                    _ => Block::Quote,
                };
                let collapsed = range.is_empty();
                let new = e.doc.toggle_block(range.clone(), kind);
                e.history.record(before, false, "");
                e.anchor = if collapsed { new.end } else { new.start };
                e.cursor = new.end;
                Outcome::Text
            }
            _ => {
                if let Some(url) = name.strip_prefix("link:") {
                    let url = url.trim();
                    if url.is_empty() {
                        return Outcome::Nothing;
                    }
                    if range.is_empty() {
                        // No selection: insert the address itself as a link.
                        let mut style = e.doc.style_at(range.start);
                        style.link = None;
                        e.doc.replace(range.clone(), url, &style);
                        let inserted = range.start..range.start + url.len();
                        e.doc.set_link(inserted.clone(), Some(url.to_owned()));
                        e.anchor = inserted.end;
                        e.cursor = inserted.end;
                        e.history.record(before, false, url);
                        return Outcome::Text;
                    }
                    e.doc.set_link(range.clone(), Some(url.to_owned()));
                    e.history.record(before, false, "");
                    return Outcome::Styled;
                }
                if let Some(c) = name.strip_prefix("colour:") {
                    let colour = u32::from_str_radix(c, 16).ok();
                    if range.is_empty() {
                        let mut typing = e.doc.typing.clone().unwrap_or_else(|| e.doc.style_at(range.start));
                        typing.colour = colour;
                        e.doc.typing = Some(typing);
                        return Outcome::Nothing;
                    }
                    e.doc.set_colour(range.clone(), colour);
                    e.history.record(before, false, "");
                    return Outcome::Styled;
                }
                if let Some(s) = name.strip_prefix("size:") {
                    let size = match s {
                        "small" => Size::Small,
                        "large" => Size::Large,
                        "huge" => Size::Huge,
                        _ => Size::Normal,
                    };
                    if range.is_empty() {
                        let mut typing = e.doc.typing.clone().unwrap_or_else(|| e.doc.style_at(range.start));
                        typing.size = size;
                        e.doc.typing = Some(typing);
                        return Outcome::Nothing;
                    }
                    e.doc.set_size(range.clone(), size);
                    e.history.record(before, false, "");
                    return Outcome::Styled;
                }
                Outcome::Nothing
            }
        }
    });
    match outcome {
        Outcome::Nothing => update_geometry(ui),
        Outcome::Styled => refresh(ui),
        Outcome::Text => {
            ui.set_editor_text(SharedString::from(with(|e| e.doc.text().to_owned())));
            push_selection(ui);
            refresh(ui);
        }
    }
    focus(ui);
}

/// Development sample exercising every format (used by the "rich" snapshot).
pub fn sample() -> RichDoc {
    let text = "太字・斜体・下線・取り消し線 and a link.\n大きな見出し\n• 箇条書きの一つ目\n• 二つ目\n1. 番号付き\n2. 二番目\n│ 引用された文章です。折り返したときに二行目以降も引用の内側に揃うかを確かめるため、少し長めの文を入れています。\n│ 二行目の引用\n\n─\n差出人: someone@example.jp\n\n-- \n山田 太郎";
    let mut d = RichDoc::from_plain(text);
    let find = |d: &RichDoc, s: &str| {
        let i = d.text().find(s).unwrap();
        i..i + s.len()
    };
    d.toggle_mark(find(&d, "太字"), Mark::Bold);
    d.toggle_mark(find(&d, "斜体"), Mark::Italic);
    d.toggle_mark(find(&d, "下線"), Mark::Underline);
    d.toggle_mark(find(&d, "取り消し線"), Mark::Strike);
    d.set_link(find(&d, "a link"), Some("https://example.org".into()));
    d.set_size(find(&d, "大きな見出し"), Size::Large);
    d.toggle_mark(find(&d, "大きな見出し"), Mark::Bold);
    d.set_colour(find(&d, "二つ目"), Some(0xc0392b));
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_doc_layout() {
        let d = original_doc("-- \n山田", "差出人: a@example.jp\n件名: 件\n", "本文");
        assert_eq!(d.text(), "\n-- \n山田\n\n─\n差出人: a@example.jp\n件名: 件\n\n本文");
        let at = d.text().find("差出人:").unwrap();
        assert!(d.style_of_char(at).bold && !d.style_of_char(at + "差出人:".len() + 1).bold);
        assert!(d.style_of_char(d.text().find("件名:").unwrap()).bold);
    }

    #[test]
    fn reply_quotes_become_quote_blocks() {
        let d = doc_from_plain("hi\n\n> a\n>\n> b");
        assert_eq!(d.text(), "hi\n\n│ a\n│ \n│ b");
    }
}
