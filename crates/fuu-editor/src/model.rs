//! Rich-text document model: one plain `String` plus sorted, non-overlapping style
//! runs over byte ranges. Lists and quotes are line prefixes (`• `, `1. `, `│ `), so
//! the text always round-trips through a plain text field (the IME bridge).

use std::ops::Range;

pub const BULLET: &str = "• ";
pub const QUOTE: &str = "│ ";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Size {
    Small,
    #[default]
    Normal,
    Large,
    Huge,
}

impl Size {
    pub fn scale(self) -> f32 {
        match self {
            Size::Small => 0.85,
            Size::Normal => 1.0,
            Size::Large => 1.3,
            Size::Huge => 1.7,
        }
    }

    fn css(self) -> Option<&'static str> {
        match self {
            Size::Small => Some("12px"),
            Size::Normal => None,
            Size::Large => Some("18px"),
            Size::Huge => Some("24px"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    /// `http(s)` or `mailto` target.
    pub link: Option<String>,
    /// 0xRRGGBB
    pub colour: Option<u32>,
    pub size: Size,
}

impl Style {
    pub fn is_plain(&self) -> bool {
        *self == Style::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Bold,
    Italic,
    Underline,
    Strike,
}

impl Mark {
    fn get(self, s: &Style) -> bool {
        match self {
            Mark::Bold => s.bold,
            Mark::Italic => s.italic,
            Mark::Underline => s.underline,
            Mark::Strike => s.strike,
        }
    }

    fn set(self, s: &mut Style, on: bool) {
        match self {
            Mark::Bold => s.bold = on,
            Mark::Italic => s.italic = on,
            Mark::Underline => s.underline = on,
            Mark::Strike => s.strike = on,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Block {
    Paragraph,
    Bullet,
    Number,
    Quote,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub range: Range<usize>,
    pub style: Style,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RichDoc {
    text: String,
    runs: Vec<Run>,
    /// Style for the next typed character at a collapsed caret (toolbar toggles).
    pub typing: Option<Style>,
}

fn floor_boundary(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous_space = true;
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            ' ' if previous_space => out.push_str("&nbsp;"),
            _ => out.push(c),
        }
        previous_space = c == ' ';
    }
    out
}

fn safe_link(url: &str) -> bool {
    let l = url.trim().to_ascii_lowercase();
    l.starts_with("https://") || l.starts_with("http://") || l.starts_with("mailto:")
}

impl RichDoc {
    pub fn from_plain(text: &str) -> Self {
        RichDoc { text: text.replace("\r\n", "\n"), runs: Vec::new(), typing: None }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn runs(&self) -> &[Run] {
        &self.runs
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Style of the character at byte `index` (default when unstyled).
    pub fn style_of_char(&self, index: usize) -> Style {
        self.runs
            .iter()
            .find(|r| r.range.start <= index && index < r.range.end)
            .map(|r| r.style.clone())
            .unwrap_or_default()
    }

    /// Style that typing at `offset` inherits: the character before (formatting
    /// carries over Enter, as in common mail editors). Links never extend by typing.
    pub fn style_at(&self, offset: usize) -> Style {
        let offset = floor_boundary(&self.text, offset);
        let before = self.text[..offset].chars().next_back();
        let mut style = match before {
            Some(c) => self.style_of_char(offset - c.len_utf8()),
            None => self.style_of_char(0),
        };
        style.link = None;
        style
    }

    fn normalise(&mut self) {
        self.runs.retain(|r| r.range.start < r.range.end && !r.style.is_plain());
        self.runs.sort_by_key(|r| r.range.start);
        let mut merged: Vec<Run> = Vec::with_capacity(self.runs.len());
        for run in self.runs.drain(..) {
            match merged.last_mut() {
                Some(last) if last.range.end == run.range.start && last.style == run.style => last.range.end = run.range.end,
                _ => merged.push(run),
            }
        }
        self.runs = merged;
    }

    /// Replace `range` with `new_text`, giving the new text `style`.
    pub fn replace(&mut self, range: Range<usize>, new_text: &str, style: &Style) {
        let start = floor_boundary(&self.text, range.start);
        let end = floor_boundary(&self.text, range.end.max(start));
        let delta = new_text.len() as isize - (end - start) as isize;
        let mut runs = Vec::with_capacity(self.runs.len() + 2);
        for run in self.runs.drain(..) {
            let Range { start: rs, end: re } = run.range.clone();
            if re <= start {
                runs.push(run);
            } else if rs >= end {
                runs.push(Run { range: (rs as isize + delta) as usize..(re as isize + delta) as usize, style: run.style });
            } else {
                if rs < start {
                    runs.push(Run { range: rs..start, style: run.style.clone() });
                }
                if re > end {
                    let shift = |v: usize| (v as isize + delta) as usize;
                    runs.push(Run { range: shift(end)..shift(re), style: run.style });
                }
            }
        }
        if !new_text.is_empty() && !style.is_plain() {
            runs.push(Run { range: start..start + new_text.len(), style: style.clone() });
        }
        self.text.replace_range(start..end, new_text);
        self.runs = runs;
        self.normalise();
    }

    /// Bring the document in line with `new_text` (the text field after an edit).
    /// Inserted text takes the typing style, or the style at the edit position.
    /// Returns the changed byte range in the new text.
    pub fn apply_text(&mut self, new_text: &str) -> Range<usize> {
        let old = self.text.clone();
        let mut prefix = old.bytes().zip(new_text.bytes()).take_while(|(a, b)| a == b).count();
        prefix = floor_boundary(&old, prefix).min(floor_boundary(new_text, prefix));
        let max_suffix = (old.len() - prefix).min(new_text.len() - prefix);
        let mut suffix = old.bytes().rev().zip(new_text.bytes().rev()).take(max_suffix).take_while(|(a, b)| a == b).count();
        while !old.is_char_boundary(old.len() - suffix) || !new_text.is_char_boundary(new_text.len() - suffix) {
            suffix -= 1;
        }
        let removed = prefix..old.len() - suffix;
        let inserted = &new_text[prefix..new_text.len() - suffix];
        let style = self.typing.take().unwrap_or_else(|| self.style_at(prefix));
        self.replace(removed, inserted, &style);
        prefix..prefix + inserted.len()
    }

    /// Change the style of every character in `range`.
    pub fn restyle(&mut self, range: Range<usize>, f: impl Fn(&mut Style)) {
        let start = floor_boundary(&self.text, range.start);
        let end = floor_boundary(&self.text, range.end);
        if start >= end {
            return;
        }
        // Split runs at the range edges, then fill gaps with default-style runs.
        let mut runs: Vec<Run> = Vec::new();
        for run in self.runs.drain(..) {
            let Range { start: rs, end: re } = run.range.clone();
            for (a, b) in [(rs, re.min(start)), (rs.max(start), re.min(end)), (rs.max(end), re)] {
                if a < b {
                    runs.push(Run { range: a..b, style: run.style.clone() });
                }
            }
        }
        runs.sort_by_key(|r| r.range.start);
        let mut cursor = start;
        let mut filled = Vec::new();
        for run in runs.iter().filter(|r| r.range.start >= start && r.range.end <= end) {
            if run.range.start > cursor {
                filled.push(Run { range: cursor..run.range.start, style: Style::default() });
            }
            cursor = run.range.end;
        }
        if cursor < end {
            filled.push(Run { range: cursor..end, style: Style::default() });
        }
        runs.extend(filled);
        for run in runs.iter_mut().filter(|r| r.range.start >= start && r.range.end <= end) {
            f(&mut run.style);
        }
        self.runs = runs;
        self.normalise();
    }

    /// Whether every non-newline character in `range` carries `mark`.
    pub fn has_mark(&self, range: Range<usize>, mark: Mark) -> bool {
        let mut any = false;
        for (i, c) in self.text[floor_boundary(&self.text, range.start)..floor_boundary(&self.text, range.end)].char_indices() {
            if c == '\n' {
                continue;
            }
            any = true;
            if !mark.get(&self.style_of_char(range.start + i)) {
                return false;
            }
        }
        any
    }

    /// Toggle a mark over a selection, or for the next typed text at a caret.
    pub fn toggle_mark(&mut self, range: Range<usize>, mark: Mark) {
        if range.start == range.end {
            let mut typing = self.typing.clone().unwrap_or_else(|| self.style_at(range.start));
            let on = !mark.get(&typing);
            mark.set(&mut typing, on);
            self.typing = Some(typing);
            return;
        }
        let on = !self.has_mark(range.clone(), mark);
        self.restyle(range, |s| mark.set(s, on));
    }

    pub fn set_link(&mut self, range: Range<usize>, url: Option<String>) {
        let url = url.filter(|u| safe_link(u));
        self.restyle(range, |s| s.link = url.clone());
    }

    pub fn set_colour(&mut self, range: Range<usize>, colour: Option<u32>) {
        if range.start == range.end {
            let mut t = self.typing.clone().unwrap_or_else(|| self.style_at(range.start));
            t.colour = colour;
            self.typing = Some(t);
        } else {
            self.restyle(range, |s| s.colour = colour);
        }
    }

    pub fn set_size(&mut self, range: Range<usize>, size: Size) {
        if range.start == range.end {
            let mut t = self.typing.clone().unwrap_or_else(|| self.style_at(range.start));
            t.size = size;
            self.typing = Some(t);
        } else {
            self.restyle(range, |s| s.size = size);
        }
    }

    pub fn clear_formatting(&mut self, range: Range<usize>) {
        if range.start == range.end {
            self.typing = Some(Style::default());
        } else {
            self.restyle(range, |s| *s = Style::default());
        }
    }

    /// Style shared by every character of a selection (for the toolbar state).
    pub fn common_style(&self, range: Range<usize>) -> Style {
        if range.start == range.end {
            return self.typing.clone().unwrap_or_else(|| self.style_at(range.start));
        }
        Style {
            bold: self.has_mark(range.clone(), Mark::Bold),
            italic: self.has_mark(range.clone(), Mark::Italic),
            underline: self.has_mark(range.clone(), Mark::Underline),
            strike: self.has_mark(range.clone(), Mark::Strike),
            link: None,
            colour: None,
            size: Size::Normal,
        }
    }

    // ───────────── Lines and blocks

    /// Byte ranges of the lines (without the newline) intersecting `range`.
    pub fn lines_in(&self, range: Range<usize>) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        let mut start = 0;
        for line in self.text.split('\n') {
            let end = start + line.len();
            if end >= range.start && start <= range.end {
                out.push(start..end);
            }
            start = end + 1;
        }
        out
    }

    /// The block kind of a line and the length of its prefix.
    pub fn block_of(line: &str) -> (Block, usize) {
        if line.starts_with(BULLET) {
            return (Block::Bullet, BULLET.len());
        }
        if line.starts_with(QUOTE) {
            return (Block::Quote, QUOTE.len());
        }
        let digits = line.bytes().take_while(u8::is_ascii_digit).count();
        if (1..=4).contains(&digits) && line[digits..].starts_with(". ") && !line.starts_with('0') {
            return (Block::Number, digits + 2);
        }
        (Block::Paragraph, 0)
    }

    /// Toggle a block kind on every line touched by `range`; numbered lists are
    /// renumbered. Returns the new selection.
    pub fn toggle_block(&mut self, range: Range<usize>, kind: Block) -> Range<usize> {
        let lines = self.lines_in(range.clone());
        let all = lines.iter().all(|l| Self::block_of(&self.text[l.clone()]).0 == kind);
        let mut shift: isize = 0;
        let (mut new_start, mut new_end) = (range.start as isize, range.end as isize);
        for (n, line) in lines.iter().enumerate() {
            let start = (line.start as isize + shift) as usize;
            let end = (line.end as isize + shift) as usize;
            let (_, prefix_len) = Self::block_of(&self.text[start..end]);
            let prefix = if all {
                String::new()
            } else {
                match kind {
                    Block::Bullet => BULLET.to_owned(),
                    Block::Quote => QUOTE.to_owned(),
                    Block::Number => format!("{}. ", n + 1),
                    Block::Paragraph => String::new(),
                }
            };
            self.replace(start..start + prefix_len, &prefix, &Style::default());
            let d = prefix.len() as isize - prefix_len as isize;
            if (start as isize) < new_start {
                new_start += d;
            }
            new_end += d;
            shift += d;
        }
        new_start.max(0) as usize..new_end.max(0) as usize
    }

    /// Enter inside a list or quote: continue the marker, or end the list on an
    /// empty item. Returns the edited text range and the new caret, or `None` for a
    /// plain newline.
    pub fn enter_in_block(&mut self, caret: usize) -> Option<usize> {
        let line = self.lines_in(caret..caret).into_iter().next()?;
        let (kind, prefix_len) = Self::block_of(&self.text[line.clone()]);
        if kind == Block::Paragraph || caret < line.start + prefix_len {
            return None;
        }
        if line.end == line.start + prefix_len {
            // Empty item: remove the marker.
            self.replace(line.start..line.end, "", &Style::default());
            return Some(line.start);
        }
        let next = match kind {
            Block::Bullet => BULLET.to_owned(),
            Block::Quote => QUOTE.to_owned(),
            Block::Number => {
                let n: usize = self.text[line.start..].split('.').next().and_then(|d| d.parse().ok()).unwrap_or(0);
                format!("{}. ", n + 1)
            }
            Block::Paragraph => unreachable!(),
        };
        let style = self.style_at(caret);
        self.replace(caret..caret, "\n", &style);
        self.replace(caret + 1..caret + 1, &next, &Style::default());
        Some(caret + 1 + next.len())
    }

    // ───────────── Export

    fn inline_html(&self, range: Range<usize>) -> String {
        let mut cuts = vec![range.start, range.end];
        for r in &self.runs {
            for b in [r.range.start, r.range.end] {
                if b > range.start && b < range.end {
                    cuts.push(b);
                }
            }
        }
        cuts.sort_unstable();
        cuts.dedup();
        let mut out = String::new();
        for w in cuts.windows(2) {
            let piece = &self.text[w[0]..w[1]];
            if piece.is_empty() {
                continue;
            }
            let style = self.style_of_char(w[0]);
            let mut html = escape(piece);
            if style.strike {
                html = format!("<s>{html}</s>");
            }
            if style.underline {
                html = format!("<u>{html}</u>");
            }
            if style.italic {
                html = format!("<em>{html}</em>");
            }
            if style.bold {
                html = format!("<strong>{html}</strong>");
            }
            let mut css = Vec::new();
            if let Some(c) = style.colour {
                css.push(format!("color:#{c:06x}"));
            }
            if let Some(s) = style.size.css() {
                css.push(format!("font-size:{s}"));
            }
            if !css.is_empty() {
                html = format!("<span style=\"{}\">{html}</span>", css.join(";"));
            }
            if let Some(link) = style.link.as_deref().filter(|l| safe_link(l)) {
                html = format!("<a href=\"{}\" style=\"color:#3b4fd1;\">{html}</a>", escape(link));
            }
            out.push_str(&html);
        }
        out
    }

    /// HTML body for sending (inline styles only).
    pub fn to_html(&self) -> String {
        #[derive(PartialEq)]
        enum Open {
            None,
            Ul,
            Ol,
            Quote,
        }
        let mut out = String::new();
        let mut open = Open::None;
        let mut start = 0;
        let mut in_signature = false;
        for line in self.text.split('\n') {
            let range = start..start + line.len();
            start = range.end + 1;
            if line == "-- " && !in_signature {
                // Everything from the signature separator on is muted.
                out.push_str(match open {
                    Open::Ul => "</ul>",
                    Open::Ol => "</ol>",
                    Open::Quote => "</blockquote>",
                    Open::None => "",
                });
                open = Open::None;
                in_signature = true;
                out.push_str("<div style=\"color:#8a877f;margin-top:16px;\"><div>--&nbsp;</div>");
                continue;
            }
            let (kind, prefix) = Self::block_of(line);
            let want = match kind {
                Block::Bullet => Open::Ul,
                Block::Number => Open::Ol,
                Block::Quote => Open::Quote,
                Block::Paragraph => Open::None,
            };
            if want != open {
                out.push_str(match open {
                    Open::Ul => "</ul>",
                    Open::Ol => "</ol>",
                    Open::Quote => "</blockquote>",
                    Open::None => "",
                });
                out.push_str(match want {
                    Open::Ul => "<ul style=\"margin:0;padding-left:24px;\">",
                    Open::Ol => "<ol style=\"margin:0;padding-left:24px;\">",
                    Open::Quote => "<blockquote style=\"margin:0 0 0 2px;padding:0 0 0 12px;border-left:3px solid #d9d6cf;color:#6b6a66;\">",
                    Open::None => "",
                });
                open = want;
            }
            let content = self.inline_html(range.start + prefix..range.end);
            let content = if content.is_empty() { "<br>".to_owned() } else { content };
            match kind {
                Block::Bullet | Block::Number => out.push_str(&format!("<li>{content}</li>")),
                _ => out.push_str(&format!("<div>{content}</div>")),
            }
        }
        out.push_str(match open {
            Open::Ul => "</ul>",
            Open::Ol => "</ol>",
            Open::Quote => "</blockquote>",
            Open::None => "",
        });
        if in_signature {
            out.push_str("</div>");
        }
        out
    }

    /// Plain-text alternative: quotes as `> `, links as `text (url)`.
    pub fn to_plain(&self) -> String {
        let mut out = String::with_capacity(self.text.len());
        let mut start = 0;
        for (i, line) in self.text.split('\n').enumerate() {
            if i > 0 {
                out.push('\n');
            }
            let range = start..start + line.len();
            start = range.end + 1;
            let (kind, prefix) = Self::block_of(line);
            let body_start = range.start + prefix;
            match kind {
                Block::Quote => out.push_str("> "),
                _ => out.push_str(&line[..prefix]),
            }
            let mut pos = body_start;
            while pos < range.end {
                let link = self.style_of_char(pos).link;
                let mut end = pos + self.text[pos..].chars().next().map_or(1, char::len_utf8);
                while end < range.end && self.style_of_char(end).link == link {
                    end += self.text[end..].chars().next().map_or(1, char::len_utf8);
                }
                let piece = &self.text[pos..end];
                out.push_str(piece);
                if let Some(url) = link {
                    let bare = url.strip_prefix("mailto:").unwrap_or(&url);
                    if piece.trim() != bare && piece.trim() != url {
                        out.push_str(&format!(" ({url})"));
                    }
                }
                pos = end;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_export_escapes_text_and_links() {
        let mut d = RichDoc::from_plain("<script>x</script> link");
        let r = d.text().find("link").unwrap();
        d.set_link(r..r + 4, Some("https://x.org/\"><b>".into()));
        let html = d.to_html();
        assert!(!html.contains("<script>") && html.contains("&lt;script&gt;"), "{html}");
        assert!(html.contains("href=\"https://x.org/&quot;&gt;&lt;b&gt;\""), "{html}");
        assert!(!html.contains("<b>"), "{html}");
    }

    #[test]
    fn signature_is_muted_in_html() {
        let html = RichDoc::from_plain("本文\n-- \n山田").to_html();
        assert!(html.contains("color:#8a877f") && html.contains("山田"), "{html}");
        assert!(!html.contains("<div>-- </div>"), "{html}");
    }

    fn bold() -> Style {
        Style { bold: true, ..Style::default() }
    }

    #[test]
    fn replace_keeps_runs_consistent() {
        let mut d = RichDoc::from_plain("hello world");
        d.restyle(0..5, |s| s.bold = true);
        assert_eq!(d.runs(), &[Run { range: 0..5, style: bold() }]);
        d.replace(5..6, "__", &Style::default());
        assert_eq!(d.text(), "hello__world");
        d.replace(0..0, ">", &Style::default());
        assert_eq!(d.runs(), &[Run { range: 1..6, style: bold() }]);
        d.replace(3..8, "", &Style::default());
        assert_eq!(d.text(), ">heworld");
        assert_eq!(d.runs(), &[Run { range: 1..3, style: bold() }]);
    }

    #[test]
    fn typing_inherits_style_and_merges() {
        let mut d = RichDoc::from_plain("ab");
        d.restyle(0..2, |s| s.bold = true);
        d.apply_text("abc");
        assert_eq!(d.runs(), &[Run { range: 0..3, style: bold() }]);
        // Formatting carries over Enter.
        d.apply_text("abc\nx");
        assert!(d.style_of_char(4).bold);
        // Plain text typed after a plain character stays plain.
        let mut p = RichDoc::from_plain("x");
        p.apply_text("xy");
        assert!(p.runs().is_empty());
    }

    #[test]
    fn apply_text_handles_multibyte_and_replacement() {
        let mut d = RichDoc::from_plain("日本語");
        d.restyle(0..3, |s| s.italic = true); // 日
        let changed = d.apply_text("日本の言語");
        assert_eq!(d.text(), "日本の言語");
        assert_eq!(&d.text()[changed], "の言");
        assert!(d.style_of_char(0).italic);
    }

    #[test]
    fn toggle_marks_on_selection_and_caret() {
        let mut d = RichDoc::from_plain("one two");
        d.toggle_mark(0..3, Mark::Bold);
        assert!(d.has_mark(0..3, Mark::Bold));
        d.toggle_mark(0..7, Mark::Bold); // not all bold → all bold
        assert!(d.has_mark(0..7, Mark::Bold));
        d.toggle_mark(0..7, Mark::Bold);
        assert!(d.runs().is_empty());
        d.toggle_mark(7..7, Mark::Italic);
        d.apply_text("one two!");
        assert!(d.style_of_char(7).italic);
        assert!(d.typing.is_none(), "typing style is consumed");
    }

    #[test]
    fn links_do_not_extend_and_reject_unsafe_urls() {
        let mut d = RichDoc::from_plain("site");
        d.set_link(0..4, Some("https://example.org".into()));
        d.apply_text("site!");
        assert_eq!(d.style_of_char(4).link, None);
        d.set_link(0..4, Some("javascript:alert(1)".into()));
        assert_eq!(d.style_of_char(0).link, None);
    }

    #[test]
    fn blocks_toggle_and_renumber() {
        let mut d = RichDoc::from_plain("a\nb\nc");
        let sel = d.toggle_block(0..5, Block::Number);
        assert_eq!(d.text(), "1. a\n2. b\n3. c");
        assert_eq!(sel, 0..d.len());
        d.toggle_block(0..d.len(), Block::Number);
        assert_eq!(d.text(), "a\nb\nc");
        d.toggle_block(2..2, Block::Bullet);
        assert_eq!(d.text(), "a\n• b\nc");
        d.toggle_block(0..d.len(), Block::Quote);
        assert_eq!(d.text(), "│ a\n│ b\n│ c");
    }

    #[test]
    fn enter_continues_and_ends_lists() {
        let mut d = RichDoc::from_plain("1. first");
        let caret = d.enter_in_block(d.len()).unwrap();
        assert_eq!(d.text(), "1. first\n2. ");
        assert_eq!(caret, d.len());
        let caret = d.enter_in_block(caret).unwrap();
        assert_eq!(d.text(), "1. first\n");
        assert_eq!(caret, d.len());
        let mut p = RichDoc::from_plain("plain");
        assert_eq!(p.enter_in_block(5), None);
    }

    #[test]
    fn exports_html_and_plain() {
        let mut d = RichDoc::from_plain("Hello <world>  x\n• one\n• two\n│ quoted\nsite");
        d.restyle(0..5, |s| s.bold = true);
        d.restyle(6..13, |s| s.colour = Some(0xc0392b));
        let site = d.text().rfind("site").unwrap();
        d.set_link(site..site + 4, Some("https://example.org/?a=1&b=2".into()));
        let html = d.to_html();
        assert!(html.contains("<strong>Hello</strong>"), "{html}");
        assert!(html.contains("&lt;world&gt;"), "{html}");
        assert!(html.contains("color:#c0392b"), "{html}");
        assert!(html.contains("&nbsp;"), "double spaces survive: {html}");
        assert!(html.contains("<ul") && html.contains("<li>one</li><li>two</li></ul>"), "{html}");
        assert!(html.contains("<blockquote") && html.contains("<div>quoted</div></blockquote>"), "{html}");
        assert!(html.contains(r#"<a href="https://example.org/?a=1&amp;b=2""#), "{html}");
        let plain = d.to_plain();
        assert!(plain.contains("• one\n• two\n> quoted"), "{plain}");
        assert!(plain.ends_with("site (https://example.org/?a=1&b=2)"), "{plain}");
    }

    #[test]
    fn common_style_reports_toolbar_state() {
        let mut d = RichDoc::from_plain("ab cd");
        d.restyle(0..5, |s| s.underline = true);
        d.restyle(0..2, |s| s.bold = true);
        let s = d.common_style(0..5);
        assert!(s.underline && !s.bold);
        assert!(d.common_style(1..1).bold, "caret inherits the style before it");
    }
}
