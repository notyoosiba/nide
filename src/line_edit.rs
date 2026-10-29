use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Reusable single-line text input widget with UTF-8 character navigation,
/// insertion, deletion, and standard editing keybindings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineEdit {
    text: String,
    cursor: usize, // character position, 0 <= cursor <= text.chars().count()
}

impl LineEdit {
    #[must_use]
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
        }
    }

    #[must_use]
    pub fn with_text(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.chars().count();
        Self { text, cursor }
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    #[must_use]
    pub fn char_count(&self) -> usize {
        self.text.chars().count()
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = self.char_count();
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    #[must_use]
    fn char_to_byte_idx(&self, char_idx: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_idx)
            .map(|(idx, _)| idx)
            .unwrap_or(self.text.len())
    }

    pub fn insert_char(&mut self, ch: char) {
        let byte_idx = self.char_to_byte_idx(self.cursor);
        self.text.insert(byte_idx, ch);
        self.cursor += 1;
    }

    pub fn insert_str(&mut self, s: &str) {
        let byte_idx = self.char_to_byte_idx(self.cursor);
        self.text.insert_str(byte_idx, s);
        self.cursor += s.chars().count();
    }

    pub fn delete_prev_char(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let start_byte = self.char_to_byte_idx(self.cursor - 1);
        let end_byte = self.char_to_byte_idx(self.cursor);
        self.text.drain(start_byte..end_byte);
        self.cursor -= 1;
        true
    }

    pub fn delete_next_char(&mut self) -> bool {
        if self.cursor >= self.char_count() {
            return false;
        }
        let start_byte = self.char_to_byte_idx(self.cursor);
        let end_byte = self.char_to_byte_idx(self.cursor + 1);
        self.text.drain(start_byte..end_byte);
        true
    }

    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.char_count() {
            self.cursor += 1;
        }
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.char_count();
    }

    pub fn delete_to_start(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let byte_idx = self.char_to_byte_idx(self.cursor);
        self.text.drain(0..byte_idx);
        self.cursor = 0;
    }

    pub fn delete_to_end(&mut self) {
        let byte_idx = self.char_to_byte_idx(self.cursor);
        self.text.truncate(byte_idx);
    }

    pub fn delete_prev_word(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let chars: Vec<char> = self.text.chars().collect();
        let mut new_cursor = self.cursor;
        // Skip non-alphanumeric trailing separators
        while new_cursor > 0 && !chars[new_cursor - 1].is_alphanumeric() {
            new_cursor -= 1;
        }
        // Skip word characters
        while new_cursor > 0 && chars[new_cursor - 1].is_alphanumeric() {
            new_cursor -= 1;
        }
        let start_byte = self.char_to_byte_idx(new_cursor);
        let end_byte = self.char_to_byte_idx(self.cursor);
        self.text.drain(start_byte..end_byte);
        self.cursor = new_cursor;
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Left => {
                self.move_left();
                true
            }
            KeyCode::Right => {
                self.move_right();
                true
            }
            KeyCode::Home => {
                self.move_home();
                true
            }
            KeyCode::End => {
                self.move_end();
                true
            }
            KeyCode::Backspace => self.delete_prev_char(),
            KeyCode::Delete => self.delete_next_char(),
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_home();
                true
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_end();
                true
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.delete_to_start();
                true
            }
            KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.delete_to_end();
                true
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.delete_prev_word();
                true
            }
            KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.delete_prev_char();
                true
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.delete_next_char();
                true
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty()
                    || key.modifiers == KeyModifiers::SHIFT
                    || key.modifiers == KeyModifiers::NONE =>
            {
                self.insert_char(c);
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_edit_basic_insert_and_cursor() {
        let mut le = LineEdit::new();
        assert_eq!(le.text(), "");
        assert_eq!(le.cursor(), 0);

        le.insert_char('h');
        le.insert_char('i');
        assert_eq!(le.text(), "hi");
        assert_eq!(le.cursor(), 2);

        le.move_left();
        assert_eq!(le.cursor(), 1);
        le.insert_char('e');
        assert_eq!(le.text(), "hei");
        assert_eq!(le.cursor(), 2);

        le.move_home();
        assert_eq!(le.cursor(), 0);
        le.insert_char('a');
        assert_eq!(le.text(), "ahei");
        assert_eq!(le.cursor(), 1);

        le.move_end();
        assert_eq!(le.cursor(), 4);
    }

    #[test]
    fn test_line_edit_delete_operations() {
        let mut le = LineEdit::with_text("abcdef");
        assert_eq!(le.cursor(), 6);

        assert!(le.delete_prev_char());
        assert_eq!(le.text(), "abcde");
        assert_eq!(le.cursor(), 5);

        le.move_home();
        assert!(!le.delete_prev_char());
        assert!(le.delete_next_char());
        assert_eq!(le.text(), "bcde");
        assert_eq!(le.cursor(), 0);

        le.set_text("foo/bar/baz.rs");
        le.delete_prev_word();
        assert_eq!(le.text(), "foo/bar/baz.");
        le.delete_prev_word();
        assert_eq!(le.text(), "foo/bar/");
        le.delete_to_start();
        assert_eq!(le.text(), "");
        assert_eq!(le.cursor(), 0);
    }

    #[test]
    fn test_line_edit_utf8_handling() {
        let mut le = LineEdit::with_text("🦀 hello");
        assert_eq!(le.char_count(), 7);
        assert_eq!(le.cursor(), 7);

        le.move_home();
        le.move_right(); // after 🦀
        assert_eq!(le.cursor(), 1);

        le.delete_prev_char(); // deletes 🦀
        assert_eq!(le.text(), " hello");
        assert_eq!(le.cursor(), 0);

        le.insert_char('漢');
        assert_eq!(le.text(), "漢 hello");
        assert_eq!(le.cursor(), 1);
    }

    #[test]
    fn test_line_edit_key_handling() {
        let mut le = LineEdit::new();
        let char_x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        assert!(le.handle_key(&char_x));
        assert_eq!(le.text(), "x");

        let left = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
        assert!(le.handle_key(&left));
        assert_eq!(le.cursor(), 0);

        let delete = KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE);
        assert!(le.handle_key(&delete));
        assert_eq!(le.text(), "");
    }
}
