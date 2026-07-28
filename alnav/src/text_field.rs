//! Single-line editable text with a char-index caret (readline subset).

use std::ops::Deref;

/// Owned text + caret. `cursor` is a Unicode scalar index into `text` (not bytes).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextField {
    text: String,
    cursor: usize,
}

impl TextField {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cursor placed at the end of `text`.
    pub fn from_text(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.chars().count();
        Self { text, cursor }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    /// Replace contents; cursor moves to the end.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = self.text.chars().count();
    }

    /// Take ownership of the string and reset to empty.
    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    /// Split at the caret: `(before, after)`.
    pub fn split_at_cursor(&self) -> (&str, &str) {
        let byte = self.byte_index(self.cursor);
        (&self.text[..byte], &self.text[byte..])
    }

    pub fn insert(&mut self, c: char) {
        let byte = self.byte_index(self.cursor);
        self.text.insert(byte, c);
        self.cursor += 1;
    }

    /// Delete one char left of the caret. Returns `true` if a char was removed.
    pub fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let end = self.byte_index(self.cursor);
        let start = self.byte_index(self.cursor - 1);
        self.text.replace_range(start..end, "");
        self.cursor -= 1;
        true
    }

    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        let n = self.text.chars().count();
        if self.cursor < n {
            self.cursor += 1;
        }
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.text.chars().count();
    }

    /// Ctrl-U: delete from start through the char before the caret.
    pub fn kill_to_start(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let byte = self.byte_index(self.cursor);
        self.text.replace_range(..byte, "");
        self.cursor = 0;
    }

    /// Ctrl-Backspace (New/Edit): delete previous word.
    ///
    /// 1) eat whitespace/punctuation immediately left of the caret  
    /// 2) eat a run of word chars (`alphanumeric` or `_`)
    pub fn kill_word_back(&mut self) {
        while self.cursor > 0 {
            let prev = self.char_before_cursor();
            if is_separator(prev) {
                let _ = self.backspace();
            } else {
                break;
            }
        }
        while self.cursor > 0 {
            let prev = self.char_before_cursor();
            if is_word_char(prev) {
                let _ = self.backspace();
            } else {
                break;
            }
        }
    }

    fn char_before_cursor(&self) -> char {
        debug_assert!(self.cursor > 0);
        self.text.chars().nth(self.cursor - 1).unwrap()
    }

    fn byte_index(&self, char_index: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_index)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len())
    }
}

impl Deref for TextField {
    type Target = str;

    fn deref(&self) -> &str {
        &self.text
    }
}

impl From<&str> for TextField {
    fn from(s: &str) -> Self {
        Self::from_text(s)
    }
}

impl From<String> for TextField {
    fn from(s: String) -> Self {
        Self::from_text(s)
    }
}

impl PartialEq<&str> for TextField {
    fn eq(&self, other: &&str) -> bool {
        self.text == *other
    }
}

impl PartialEq<str> for TextField {
    fn eq(&self, other: &str) -> bool {
        self.text == other
    }
}

impl PartialEq<String> for TextField {
    fn eq(&self, other: &String) -> bool {
        &self.text == other
    }
}

fn is_separator(c: char) -> bool {
    c.is_whitespace() || c.is_ascii_punctuation()
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_at_end_and_middle() {
        let mut f = TextField::new();
        f.insert('a');
        f.insert('c');
        f.move_left();
        f.insert('b');
        assert_eq!(f.as_str(), "abc");
        assert_eq!(f.cursor(), 2);
    }

    #[test]
    fn backspace_mid_and_at_start() {
        let mut f = TextField::from_text("ab中");
        f.home();
        assert!(!f.backspace());
        f.end();
        assert!(f.backspace());
        assert_eq!(f.as_str(), "ab");
        f.move_left();
        assert!(f.backspace());
        assert_eq!(f.as_str(), "b");
        assert_eq!(f.cursor(), 0);
    }

    #[test]
    fn home_end_arrows() {
        let mut f = TextField::from_text("hi");
        f.home();
        assert_eq!(f.cursor(), 0);
        f.move_right();
        assert_eq!(f.cursor(), 1);
        f.end();
        assert_eq!(f.cursor(), 2);
        f.move_right();
        assert_eq!(f.cursor(), 2);
    }

    #[test]
    fn kill_to_start() {
        let mut f = TextField::from_text("hello");
        f.move_left();
        f.move_left();
        f.kill_to_start();
        assert_eq!(f.as_str(), "lo");
        assert_eq!(f.cursor(), 0);
    }

    #[test]
    fn kill_word_back_whitespace_and_punct() {
        let mut f = TextField::from_text("foo bar");
        f.kill_word_back();
        assert_eq!(f.as_str(), "foo ");
        f.kill_word_back();
        assert_eq!(f.as_str(), "");

        let mut f = TextField::from_text("foo.bar");
        f.kill_word_back();
        assert_eq!(f.as_str(), "foo.");
        f.kill_word_back();
        assert_eq!(f.as_str(), "");
    }

    #[test]
    fn unicode_char_boundaries() {
        let mut f = TextField::from_text("a测b");
        f.home();
        f.move_right();
        f.insert('X');
        assert_eq!(f.as_str(), "aX测b");
        f.move_right();
        assert!(f.backspace());
        assert_eq!(f.as_str(), "aXb");
    }

    #[test]
    fn split_at_cursor() {
        let mut f = TextField::from_text("ab");
        f.move_left();
        let (a, b) = f.split_at_cursor();
        assert_eq!((a, b), ("a", "b"));
    }

    #[test]
    fn take_and_set_text() {
        let mut f = TextField::from_text("x");
        assert_eq!(f.take(), "x");
        assert!(f.is_empty());
        f.set_text("yz");
        assert_eq!(f.as_str(), "yz");
        assert_eq!(f.cursor(), 2);
    }
}
