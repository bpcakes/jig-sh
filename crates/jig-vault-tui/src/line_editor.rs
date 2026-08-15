use jig_tui::sanitize_text;
use ratatui::text::Line;

pub(crate) const METADATA_INPUT_LIMIT: usize = 128 * 1024;
pub(crate) const SEARCH_INPUT_LIMIT: usize = 256;
pub(crate) const COMMAND_INPUT_LIMIT: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LineEditor {
    text: String,
    cursor: usize,
    max_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LineEdit {
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    WordLeft,
    WordRight,
    DeleteWordLeft,
    Clear,
}

impl LineEdit {
    pub(crate) const fn changes_text(self) -> bool {
        matches!(
            self,
            Self::Backspace | Self::Delete | Self::DeleteWordLeft | Self::Clear
        )
    }
}

impl LineEditor {
    pub(crate) const fn new(max_bytes: usize) -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            max_bytes,
        }
    }

    pub(crate) const fn metadata() -> Self {
        Self::new(METADATA_INPUT_LIMIT)
    }

    pub(crate) const fn search() -> Self {
        Self::new(SEARCH_INPUT_LIMIT)
    }

    pub(crate) const fn command() -> Self {
        Self::new(COMMAND_INPUT_LIMIT)
    }

    pub(crate) fn prefilled_metadata(text: String) -> Self {
        let mut editor = Self::metadata();
        assert!(
            editor.insert(&text),
            "validated vault metadata exceeds the interactive editor limit"
        );
        editor
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.text
    }

    #[cfg(test)]
    pub(crate) const fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub(crate) fn insert(&mut self, value: &str) -> bool {
        let value = value
            .chars()
            .filter(|character| !character.is_control())
            .collect::<String>();
        let Some(next_len) = self.text.len().checked_add(value.len()) else {
            return false;
        };
        if next_len > self.max_bytes {
            return false;
        }
        self.text.insert_str(self.cursor, &value);
        self.cursor += value.len();
        true
    }

    pub(crate) fn backspace(&mut self) {
        let Some(previous) = previous_boundary(&self.text, self.cursor) else {
            return;
        };
        self.text.drain(previous..self.cursor);
        self.cursor = previous;
    }

    pub(crate) fn delete(&mut self) {
        let Some(next) = next_boundary(&self.text, self.cursor) else {
            return;
        };
        self.text.drain(self.cursor..next);
    }

    pub(crate) fn move_left(&mut self) {
        if let Some(previous) = previous_boundary(&self.text, self.cursor) {
            self.cursor = previous;
        }
    }

    pub(crate) fn move_right(&mut self) {
        if let Some(next) = next_boundary(&self.text, self.cursor) {
            self.cursor = next;
        }
    }

    pub(crate) const fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub(crate) fn move_end(&mut self) {
        self.cursor = self.text.len();
    }

    pub(crate) fn move_word_left(&mut self) {
        let mut cursor = self.cursor;
        while let Some(previous) = previous_boundary(&self.text, cursor) {
            if !char_at(&self.text, previous).is_some_and(char::is_whitespace) {
                break;
            }
            cursor = previous;
        }
        while let Some(previous) = previous_boundary(&self.text, cursor) {
            if char_at(&self.text, previous).is_some_and(char::is_whitespace) {
                break;
            }
            cursor = previous;
        }
        self.cursor = cursor;
    }

    pub(crate) fn move_word_right(&mut self) {
        let mut cursor = self.cursor;
        while let Some(character) = char_at(&self.text, cursor) {
            if !character.is_whitespace() {
                break;
            }
            cursor += character.len_utf8();
        }
        while let Some(character) = char_at(&self.text, cursor) {
            if character.is_whitespace() {
                break;
            }
            cursor += character.len_utf8();
        }
        self.cursor = cursor;
    }

    pub(crate) fn delete_word_left(&mut self) {
        let end = self.cursor;
        self.move_word_left();
        self.text.drain(self.cursor..end);
    }

    pub(crate) fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    pub(crate) fn apply(&mut self, edit: LineEdit) {
        match edit {
            LineEdit::Backspace => self.backspace(),
            LineEdit::Delete => self.delete(),
            LineEdit::Left => self.move_left(),
            LineEdit::Right => self.move_right(),
            LineEdit::Home => self.move_home(),
            LineEdit::End => self.move_end(),
            LineEdit::WordLeft => self.move_word_left(),
            LineEdit::WordRight => self.move_word_right(),
            LineEdit::DeleteWordLeft => self.delete_word_left(),
            LineEdit::Clear => self.clear(),
        }
    }

    pub(crate) fn window(&self, max_width: usize) -> LineWindow {
        if max_width == 0 {
            return LineWindow::default();
        }
        if max_width == 1 {
            return LineWindow {
                cursor_column: 0,
                ..LineWindow::default()
            };
        }
        let content_budget = max_width.saturating_sub(1);
        let mut start = window_start(&self.text, self.cursor, content_budget);
        let clipped_left = start > 0;
        let budget_after_left = content_budget.saturating_sub(usize::from(clipped_left));
        if clipped_left {
            start = window_start(&self.text, self.cursor, budget_after_left);
        }

        let mut used = display_width(&self.text[start..self.cursor]);
        let mut end = self.cursor;
        for (offset, character) in self.text[self.cursor..].char_indices() {
            let width = character_width(character);
            if used.saturating_add(width) > budget_after_left {
                break;
            }
            used += width;
            end = self.cursor + offset + character.len_utf8();
        }
        let clipped_right = end < self.text.len() && (!clipped_left || max_width >= 3);
        if clipped_right {
            let budget = budget_after_left.saturating_sub(1);
            while end > self.cursor && used > budget {
                let previous = previous_boundary(&self.text, end).unwrap_or(self.cursor);
                used =
                    used.saturating_sub(char_at(&self.text, previous).map_or(0, character_width));
                end = previous;
            }
        }

        LineWindow {
            before: self.text[start..self.cursor].to_owned(),
            after: self.text[self.cursor..end].to_owned(),
            clipped_left,
            clipped_right,
            cursor_column: usize::from(clipped_left)
                + display_width(&self.text[start..self.cursor]),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct LineWindow {
    pub(crate) before: String,
    pub(crate) after: String,
    pub(crate) clipped_left: bool,
    pub(crate) clipped_right: bool,
    pub(crate) cursor_column: usize,
}

fn previous_boundary(value: &str, cursor: usize) -> Option<usize> {
    value[..cursor]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
}

fn next_boundary(value: &str, cursor: usize) -> Option<usize> {
    value[cursor..]
        .chars()
        .next()
        .map(|character| cursor + character.len_utf8())
}

fn char_at(value: &str, cursor: usize) -> Option<char> {
    value[cursor..].chars().next()
}

fn window_start(value: &str, cursor: usize, budget: usize) -> usize {
    let mut start = cursor;
    let mut used = 0usize;
    for (index, character) in value[..cursor].char_indices().rev() {
        let width = character_width(character);
        if used.saturating_add(width) > budget {
            break;
        }
        used += width;
        start = index;
    }
    start
}

fn character_width(character: char) -> usize {
    let mut encoded = [0; 4];
    display_width(character.encode_utf8(&mut encoded))
}

fn display_width(value: &str) -> usize {
    Line::from(sanitize_text(value)).width()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insertion_and_deletion_preserve_unicode_boundaries() {
        let mut editor = LineEditor::new(32);
        assert!(editor.insert("a界c"));
        editor.move_left();
        editor.move_left();
        assert_eq!(editor.cursor(), 1);
        assert!(editor.insert("b"));
        assert_eq!(editor.as_str(), "ab界c");
        editor.delete();
        assert_eq!(editor.as_str(), "abc");
        editor.backspace();
        assert_eq!(editor.as_str(), "ac");
    }

    #[test]
    fn insertion_is_filtered_and_bounded_atomically() {
        let mut editor = LineEditor::new(5);
        assert!(editor.insert("ab\nc"));
        assert_eq!(editor.as_str(), "abc");
        assert!(!editor.insert("界"));
        assert_eq!(editor.as_str(), "abc");
        assert_eq!(editor.cursor(), 3);
    }

    #[test]
    fn word_editing_uses_shell_style_whitespace_boundaries() {
        let mut editor = LineEditor::new(64);
        assert!(editor.insert("alpha beta gamma"));
        editor.move_word_left();
        assert_eq!(editor.cursor(), "alpha beta ".len());
        editor.delete_word_left();
        assert_eq!(editor.as_str(), "alpha gamma");
        editor.move_home();
        editor.move_word_right();
        assert_eq!(editor.cursor(), "alpha".len());
    }

    #[test]
    fn viewport_keeps_the_cursor_visible_at_both_edges() {
        let mut editor = LineEditor::new(64);
        assert!(editor.insert("0123456789"));
        let end = editor.window(6);
        assert!(end.clipped_left);
        assert!(!end.clipped_right);
        assert_eq!(end.cursor_column, 5);
        assert!(end.before.ends_with("6789"));

        editor.move_home();
        let home = editor.window(6);
        assert!(!home.clipped_left);
        assert!(home.clipped_right);
        assert_eq!(home.cursor_column, 0);
        assert!(home.after.starts_with("0123"));
    }
}
