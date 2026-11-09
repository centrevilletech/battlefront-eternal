//! Native single-line editing shared by shell fields and battle chat.
//! Text comes from the window system, independently of physical game bindings.

#[derive(Default)]
pub(super) struct TextInput {
    field: Option<&'static str>,
    cursor: usize,
    anchor: usize,
}

impl TextInput {
    /// Byte positions for native field painters; editing stays in this owner.
    #[allow(dead_code)]
    pub fn selection(&self) -> (usize, usize) {
        (self.cursor, self.anchor)
    }

    pub fn focus(&mut self, field: Option<&'static str>, text: &str) {
        if self.field != field {
            self.field = field;
            self.cursor = text.len();
            self.anchor = self.cursor;
        }
        // A submit or external update can replace the draft between events.
        self.cursor = self.cursor.min(text.len());
        while !text.is_char_boundary(self.cursor) {
            self.cursor -= 1;
        }
        self.anchor = self.anchor.min(text.len());
        while !text.is_char_boundary(self.anchor) {
            self.anchor -= 1;
        }
    }

    pub fn insert(&mut self, text: &mut String, value: &str, limit: usize) {
        let range = self.cursor.min(self.anchor)..self.cursor.max(self.anchor);
        let available = limit.saturating_sub(
            text[..range.start].chars().count() + text[range.end..].chars().count(),
        );
        let value: String = value
            .chars()
            .filter(|c| !c.is_control())
            .take(available)
            .collect();
        text.replace_range(range.clone(), &value);
        self.cursor = range.start + value.len();
        self.anchor = self.cursor;
    }

    pub fn key(&mut self, text: &mut String, key: &str, shift: bool, command: bool) -> bool {
        let previous = |at: usize| text[..at].char_indices().next_back().map_or(0, |(i, _)| i);
        let next = |at: usize| text[at..].chars().next().map_or(at, |c| at + c.len_utf8());
        let selected = self.cursor != self.anchor;
        match key {
            "a" | "A" if command => {
                self.anchor = 0;
                self.cursor = text.len();
                return true;
            }
            "ArrowLeft" => {
                self.cursor = if selected && !shift {
                    self.cursor.min(self.anchor)
                } else {
                    previous(self.cursor)
                }
            }
            "ArrowRight" => {
                self.cursor = if selected && !shift {
                    self.cursor.max(self.anchor)
                } else {
                    next(self.cursor)
                }
            }
            "Home" => self.cursor = 0,
            "End" => self.cursor = text.len(),
            "Backspace" | "Delete" => {
                if !selected {
                    self.anchor = if key == "Backspace" {
                        previous(self.cursor)
                    } else {
                        next(self.cursor)
                    };
                }
                self.insert(text, "", usize::MAX);
                return true;
            }
            _ => return false,
        }
        if !shift {
            self.anchor = self.cursor;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_selection_replacement_and_forward_delete() {
        let mut edit = TextInput::default();
        let mut value = "aé界".to_string();
        edit.focus(Some("profile"), &value);
        edit.key(&mut value, "ArrowLeft", false, false);
        edit.key(&mut value, "Backspace", false, false);
        assert_eq!(value, "a界");
        edit.key(&mut value, "Delete", false, false);
        assert_eq!(value, "a");
        edit.key(&mut value, "a", false, true);
        edit.insert(&mut value, "é界\nxyz", 3);
        assert_eq!(value, "é界x");
        edit.key(&mut value, "Home", false, false);
        edit.key(&mut value, "ArrowRight", true, false);
        edit.insert(&mut value, "A", 3);
        assert_eq!(value, "A界x");
    }
}
