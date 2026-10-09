//! Word wrapping for detail lines: continuation rows hang under the line's
//! value, long unbreakable words fill the row before breaking, and wide
//! characters never overflow it.

use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// A detail line and the column its wrapped continuation rows start at.
pub(super) struct Detail {
    pub(super) line: Line<'static>,
    pub(super) hang: usize,
}

impl Detail {
    pub(super) fn new(line: Line<'static>, hang: usize) -> Self {
        Self { line, hang }
    }

    /// `label  value`, with the label muted and padded to `label_width`.
    pub(super) fn labeled(
        label: &str,
        value: &str,
        label_width: usize,
        label_style: Style,
    ) -> Self {
        let label = super::pad(label, label_width + 2);
        Self {
            hang: label.width(),
            line: Line::from(vec![
                Span::styled(label, label_style),
                Span::raw(value.to_owned()),
            ]),
        }
    }
}

/// Word-wraps a detail to `width`, starting continuation rows at its hang
/// column (at most half the width) so wrapped values stay under their label.
pub(super) fn wrap(detail: Detail, width: usize) -> Vec<Line<'static>> {
    if width == 0 || detail.line.width() <= width {
        return vec![detail.line];
    }
    let hang = detail.hang.min(width / 2);
    let line_style = detail.line.style;
    let mut rows = Vec::new();
    let mut row = WrappedRow::new(0);
    for span in detail.line.spans {
        for word in span.content.split_inclusive(' ') {
            let mut word = word;
            while !word.is_empty() {
                if row.width + word.trim_end().width() <= width {
                    row.push(word, span.style);
                    break;
                }
                let fits_a_fresh_row = word.trim_end().width() <= width - hang;
                if row.has_text && (fits_a_fresh_row || row.width >= width) {
                    rows.push(row.finish().style(line_style));
                    row = WrappedRow::new(hang);
                    continue;
                }
                // The word is wider than a whole row; break it at this row's end.
                let (head, tail) = split_at_width(word, width.saturating_sub(row.width));
                if head.is_empty() && row.has_text {
                    rows.push(row.finish().style(line_style));
                    row = WrappedRow::new(hang);
                    continue;
                }
                let (head, tail) = if head.is_empty() {
                    // Not even one character fits an empty row; take it anyway.
                    word.split_at(word.chars().next().map_or(0, char::len_utf8))
                } else {
                    (head, tail)
                };
                row.push(head, span.style);
                rows.push(row.finish().style(line_style));
                row = WrappedRow::new(hang);
                word = tail;
            }
        }
    }
    if row.has_text {
        rows.push(row.finish().style(line_style));
    }
    rows
}

struct WrappedRow {
    spans: Vec<Span<'static>>,
    width: usize,
    has_text: bool,
}

impl WrappedRow {
    fn new(indent: usize) -> Self {
        Self {
            spans: if indent == 0 {
                Vec::new()
            } else {
                vec![Span::raw(" ".repeat(indent))]
            },
            width: indent,
            has_text: false,
        }
    }

    fn push(&mut self, text: &str, style: Style) {
        self.width += text.width();
        self.has_text = true;
        match self.spans.last_mut() {
            Some(last) if last.style == style => last.content.to_mut().push_str(text),
            _ => self.spans.push(Span::styled(text.to_owned(), style)),
        }
    }

    fn finish(mut self) -> Line<'static> {
        while let Some(last) = self.spans.last_mut() {
            let trimmed = last.content.trim_end().len();
            if trimmed > 0 {
                last.content.to_mut().truncate(trimmed);
                break;
            }
            self.spans.pop();
        }
        Line::from(self.spans)
    }
}

/// Splits off the longest prefix that fits `width`.
fn split_at_width(word: &str, width: usize) -> (&str, &str) {
    let mut used = 0;
    let mut end = 0;
    for (offset, character) in word.char_indices() {
        let character_width = character.width().unwrap_or(0);
        if used + character_width > width {
            break;
        }
        used += character_width;
        end = offset + character.len_utf8();
    }
    word.split_at(end)
}
