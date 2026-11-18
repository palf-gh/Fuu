//! Layout, painting and geometry for the rich editor (parley + vello_cpu).
//!
//! The document is laid out with parley at the editor width; glyphs and text
//! decorations are painted into a premultiplied RGBA buffer the host shows as an
//! image. The caret and selection are geometry for the host to draw on top, so
//! moving the caret never repaints the text. While an IME composition is active
//! its text is inserted (underlined) at the caret for display only.

use std::ops::Range;

use anyrender::{render_to_buffer, Glyph, PaintScene};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use kurbo::{Affine, Rect};
use parley::{
    Affinity, Alignment, AlignmentOptions, Cursor, FontContext, FontStyle, FontWeight, GenericFamily, Layout,
    LayoutContext, LineHeight, PositionedLayoutItem, Selection, StyleProperty,
};
use peniko::{Color, Fill};

use crate::model::{Block, RichDoc};

/// Text colour carried through the layout (parley "brush").
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Ink(pub [u8; 4]);

impl Ink {
    fn colour(self) -> Color {
        Color::from_rgba8(self.0[0], self.0[1], self.0[2], self.0[3])
    }

    pub fn from_rgb(rgb: u32) -> Self {
        Ink([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255])
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub text: Ink,
    pub muted: Ink,
    pub link: Ink,
    pub quote_bar: Ink,
    /// Base font size in logical pixels.
    pub font_size: f32,
}

impl Palette {
    pub fn light() -> Self {
        Palette {
            text: Ink::from_rgb(0x1c1c1a),
            muted: Ink::from_rgb(0x8a877f),
            link: Ink::from_rgb(0x3b4fd1),
            quote_bar: Ink::from_rgb(0xd9d6cf),
            font_size: 14.0,
        }
    }

    pub fn dark() -> Self {
        Palette {
            text: Ink::from_rgb(0xececef),
            muted: Ink::from_rgb(0x8e8e95),
            link: Ink::from_rgb(0x9aa6ff),
            quote_bar: Ink::from_rgb(0x3a3a40),
            font_size: 14.0,
        }
    }
}

/// A rectangle in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rectf {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub struct Painted {
    pub width: u32,
    pub height: u32,
    pub rgba_premultiplied: Vec<u8>,
}

pub struct EditorView {
    font_cx: FontContext,
    layout_cx: LayoutContext<Ink>,
    layout: Layout<Ink>,
    /// Text laid out (document text plus any IME composition).
    display: String,
    preedit: Option<(usize, usize)>,
    scale: f32,
    width: f32,
    palette: Palette,
}

impl Default for EditorView {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorView {
    pub fn new() -> Self {
        EditorView {
            font_cx: FontContext::new(),
            layout_cx: LayoutContext::new(),
            layout: Layout::new(),
            display: String::new(),
            preedit: None,
            scale: 1.0,
            width: 600.0,
            palette: Palette::light(),
        }
    }

    /// Document offset → display offset (shifted past an active composition).
    fn to_display(&self, offset: usize) -> usize {
        match self.preedit {
            Some((at, len)) if offset >= at => offset + len,
            _ => offset,
        }
    }

    /// Display offset → document offset (inside a composition maps to its start).
    fn to_doc(&self, offset: usize) -> usize {
        match self.preedit {
            Some((at, len)) if offset >= at + len => offset - len,
            Some((at, _)) if offset > at => at,
            _ => offset,
        }
    }

    /// Lay the document out at `width` logical pixels.
    pub fn relayout(&mut self, doc: &RichDoc, preedit: Option<(usize, &str)>, width: f32, scale: f32, palette: Palette) {
        self.scale = scale.max(0.5);
        self.width = width.max(40.0);
        self.palette = palette;
        let mut display = doc.text().to_owned();
        self.preedit = None;
        if let Some((at, text)) = preedit.filter(|(_, t)| !t.is_empty()) {
            let at = at.min(display.len());
            if display.is_char_boundary(at) {
                display.insert_str(at, text);
                self.preedit = Some((at, text.len()));
            }
        }
        let preedit_span = self.preedit;
        let to_display = move |offset: usize| match preedit_span {
            Some((at, len)) if offset >= at => offset + len,
            _ => offset,
        };
        let shifted = move |r: &Range<usize>| to_display(r.start)..to_display(r.end);
        let base = palette.font_size;
        let mut builder = self.layout_cx.ranged_builder(&mut self.font_cx, &display, self.scale, true);
        builder.push_default(StyleProperty::from(GenericFamily::SansSerif));
        builder.push_default(StyleProperty::FontSize(base));
        builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(1.6)));
        builder.push_default(StyleProperty::Brush(palette.text));
        // Block prefixes and quotes are muted.
        let mut start = 0;
        for line in doc.text().split('\n') {
            let range = start..start + line.len();
            start = range.end + 1;
            let (kind, prefix) = RichDoc::block_of(line);
            match kind {
                Block::Quote => builder.push(StyleProperty::Brush(palette.muted), shifted(&range)),
                Block::Bullet | Block::Number => builder.push(StyleProperty::Brush(palette.muted), shifted(&(range.start..range.start + prefix))),
                Block::Paragraph => {}
            }
        }
        for run in doc.runs() {
            let r = shifted(&run.range);
            let s = &run.style;
            if s.bold {
                builder.push(StyleProperty::FontWeight(FontWeight::BOLD), r.clone());
            }
            if s.italic {
                builder.push(StyleProperty::FontStyle(FontStyle::Italic), r.clone());
            }
            if s.underline || s.link.is_some() {
                builder.push(StyleProperty::Underline(true), r.clone());
            }
            if s.strike {
                builder.push(StyleProperty::Strikethrough(true), r.clone());
            }
            if s.size != crate::model::Size::Normal {
                builder.push(StyleProperty::FontSize(base * s.size.scale()), r.clone());
            }
            if s.link.is_some() {
                builder.push(StyleProperty::Brush(palette.link), r.clone());
            } else if let Some(c) = s.colour {
                builder.push(StyleProperty::Brush(Ink::from_rgb(c)), r.clone());
            }
        }
        if let Some((at, len)) = preedit_span {
            builder.push(StyleProperty::Underline(true), at..at + len);
        }
        builder.build_into(&mut self.layout, &display);
        self.layout.break_all_lines(Some(self.width * self.scale));
        self.layout.align(Alignment::Start, AlignmentOptions::default());
        self.display = display;
    }

    /// Content height in logical pixels (at least one line).
    pub fn height(&self) -> f32 {
        (self.layout.height() / self.scale).max(self.palette.font_size * 1.6)
    }

    /// Paint glyphs, decorations and quote bars (transparent background).
    pub fn paint(&self) -> Painted {
        let width = (self.width * self.scale).ceil().max(1.0) as u32;
        let height = (self.height() * self.scale).ceil().max(1.0) as u32;
        let quote_lines = self.quote_line_ranges();
        let bar = self.palette.quote_bar.colour();
        let rgba = render_to_buffer::<VelloCpuImageRenderer, _>(
            |scene| {
                for line in self.layout.lines() {
                    let m = line.metrics();
                    let line_range = line.text_range();
                    if quote_lines.iter().any(|q| q.start <= line_range.start && line_range.start <= q.end) {
                        let x = 1.0 * f64::from(self.scale);
                        let rect = Rect::new(x, f64::from(m.block_min_coord), x + 3.0 * f64::from(self.scale), f64::from(m.block_max_coord));
                        scene.fill(Fill::NonZero, Affine::IDENTITY, bar, None, &rect);
                    }
                    for item in line.items() {
                        let PositionedLayoutItem::GlyphRun(glyph_run) = item else { continue };
                        let style = glyph_run.style();
                        let run = glyph_run.run();
                        let colour = style.brush.colour();
                        let coords: Vec<i16> = run.normalized_coords().iter().map(|c| c.to_bits()).collect();
                        scene.draw_glyphs(
                            run.font(),
                            run.font_size(),
                            true,
                            &coords,
                            kurbo::Vec2::ZERO,
                            Fill::NonZero,
                            colour,
                            1.0,
                            Affine::IDENTITY,
                            None,
                            glyph_run.positioned_glyphs().map(|g| Glyph { id: g.id as _, x: g.x, y: g.y }),
                        );
                        let metrics = run.font_metrics();
                        let x0 = f64::from(glyph_run.offset());
                        let x1 = x0 + f64::from(glyph_run.advance());
                        let baseline = f64::from(glyph_run.baseline());
                        let mut decorate = |offset: f32, size: f32| {
                            let y = baseline - f64::from(offset);
                            let h = f64::from(size.max(1.0));
                            scene.fill(Fill::NonZero, Affine::IDENTITY, colour, None, &Rect::new(x0, y, x1, y + h));
                        };
                        if style.underline.is_some() {
                            decorate(metrics.underline_offset, metrics.underline_size);
                        }
                        if style.strikethrough.is_some() {
                            decorate(metrics.strikethrough_offset, metrics.strikethrough_size);
                        }
                    }
                }
            },
            width,
            height,
        );
        Painted { width, height, rgba_premultiplied: rgba }
    }

    fn quote_line_ranges(&self) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        let mut start = 0;
        for line in self.display.split('\n') {
            if line.starts_with(crate::model::QUOTE) {
                out.push(start..start + line.len());
            }
            start += line.len() + 1;
        }
        out
    }

    fn cursor(&self, offset: usize) -> Cursor {
        Cursor::from_byte_index(&self.layout, self.to_display(offset).min(self.display.len()), Affinity::Downstream)
    }

    fn selection(&self, anchor: usize, focus: usize) -> Selection {
        Selection::new(self.cursor(anchor), self.cursor(focus))
    }

    fn rect(&self, b: parley::BoundingBox) -> Rectf {
        let s = f64::from(self.scale);
        Rectf { x: (b.x0 / s) as f32, y: (b.y0 / s) as f32, w: ((b.x1 - b.x0) / s) as f32, h: ((b.y1 - b.y0) / s) as f32 }
    }

    /// Caret rectangle for a document offset (at the end of an active composition).
    pub fn caret(&self, offset: usize) -> Rectf {
        let display = match self.preedit {
            Some((at, len)) if offset == at => at + len,
            _ => self.to_display(offset),
        };
        let cursor = Cursor::from_byte_index(&self.layout, display.min(self.display.len()), Affinity::Downstream);
        let mut r = self.rect(cursor.geometry(&self.layout, 1.5 * self.scale));
        if r.h <= 0.0 {
            r.h = self.palette.font_size * 1.6;
        }
        r.w = 1.5;
        r
    }

    /// Selection rectangles between two document offsets.
    pub fn selection_rects(&self, anchor: usize, focus: usize) -> Vec<Rectf> {
        if anchor == focus {
            return Vec::new();
        }
        self.selection(anchor, focus).geometry(&self.layout).into_iter().map(|(b, _)| self.rect(b)).collect()
    }

    /// Document offset under a point (logical pixels).
    pub fn hit(&self, x: f32, y: f32) -> usize {
        let c = Cursor::from_point(&self.layout, x * self.scale, y * self.scale);
        self.to_doc(c.index())
    }

    /// Word or line under a point, as a document range.
    pub fn word_at(&self, x: f32, y: f32) -> Range<usize> {
        let s = Selection::word_from_point(&self.layout, x * self.scale, y * self.scale);
        let r = s.text_range();
        self.to_doc(r.start)..self.to_doc(r.end)
    }

    pub fn line_at(&self, x: f32, y: f32) -> Range<usize> {
        let s = Selection::hard_line_from_point(&self.layout, x * self.scale, y * self.scale);
        let r = s.text_range();
        self.to_doc(r.start)..self.to_doc(r.end)
    }

    fn from_selection(&self, s: Selection) -> (usize, usize) {
        (self.to_doc(s.anchor().index()), self.to_doc(s.focus().index()))
    }

    /// Move by visual lines (Up/Down), optionally extending the selection.
    pub fn move_lines(&self, anchor: usize, focus: usize, delta: isize, extend: bool) -> (usize, usize) {
        self.from_selection(self.selection(anchor, focus).move_lines(&self.layout, delta, extend))
    }

    pub fn line_start(&self, anchor: usize, focus: usize, extend: bool) -> (usize, usize) {
        self.from_selection(self.selection(anchor, focus).line_start(&self.layout, extend))
    }

    pub fn line_end(&self, anchor: usize, focus: usize, extend: bool) -> (usize, usize) {
        self.from_selection(self.selection(anchor, focus).line_end(&self.layout, extend))
    }

    /// Number of visual lines on one page of `height` logical pixels.
    pub fn lines_per_page(&self, height: f32) -> isize {
        ((height / (self.palette.font_size * 1.6)).floor() as isize - 1).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view_of(text: &str, width: f32) -> (RichDoc, EditorView) {
        let doc = RichDoc::from_plain(text);
        let mut v = EditorView::new();
        v.relayout(&doc, None, width, 1.0, Palette::light());
        (doc, v)
    }

    #[test]
    fn lays_out_and_paints() {
        let (_, v) = view_of("こんにちは\n2 行目 with English", 300.0);
        assert!(v.height() > 30.0, "two lines: {}", v.height());
        let p = v.paint();
        assert_eq!(p.rgba_premultiplied.len(), (p.width * p.height * 4) as usize);
        assert!(p.rgba_premultiplied.chunks(4).any(|px| px[3] > 0), "glyphs were painted");
    }

    #[test]
    fn hit_and_caret_round_trip() {
        let (doc, v) = view_of("abc\ndef", 300.0);
        let caret = v.caret(5); // between d and e
        let back = v.hit(caret.x + 0.5, caret.y + caret.h / 2.0);
        assert_eq!(back, 5);
        assert!(v.caret(4).y > v.caret(0).y, "second line is lower");
        assert_eq!(v.move_lines(1, 1, 1, false), (5, 5));
        assert_eq!(v.line_end(0, 0, false).1, 3);
        assert_eq!(v.line_start(6, 6, true), (6, 4));
        assert_eq!(v.selection_rects(0, doc.len()).len(), 2);
    }

    #[test]
    fn preedit_shifts_display_but_not_document() {
        let doc = RichDoc::from_plain("ab");
        let mut v = EditorView::new();
        v.relayout(&doc, Some((1, "にほん")), 300.0, 1.0, Palette::light());
        assert!(v.caret(1).x > v.caret(0).x + 10.0, "caret after the composition");
        let end = v.caret(2);
        assert_eq!(v.hit(end.x + 1.0, end.y + end.h / 2.0), 2);
    }

    #[test]
    fn wraps_long_lines() {
        let (_, narrow) = view_of(&"word ".repeat(60), 120.0);
        let (_, wide) = view_of(&"word ".repeat(60), 1000.0);
        assert!(narrow.height() > wide.height() * 3.0);
    }
}
