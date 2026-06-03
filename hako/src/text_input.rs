use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub enum TextInputEvent {
    Submitted(String),
    Cancelled,
    Changed,
    Ignored,
}

#[derive(Default)]
pub struct TextInput {
    value: String,
    cursor: usize,
}

impl TextInput {
    pub fn new(initial: impl Into<String>) -> Self {
        let value = initial.into();
        let cursor = value.len();
        Self { value, cursor }
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> TextInputEvent {
        match key.code {
            KeyCode::Esc => {
                self.value.clear();
                self.cursor = 0;
                TextInputEvent::Cancelled
            }
            KeyCode::Enter => TextInputEvent::Submitted(self.value.clone()),
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    let prev = self.value[..self.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.value.remove(prev);
                    self.cursor = prev;
                }
                TextInputEvent::Changed
            }
            KeyCode::Delete => {
                if self.cursor < self.value.len() {
                    self.value.remove(self.cursor);
                }
                TextInputEvent::Changed
            }
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor = self.value[..self.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
                TextInputEvent::Changed
            }
            KeyCode::Right => {
                if self.cursor < self.value.len() {
                    let ch = self.value[self.cursor..].chars().next().unwrap();
                    self.cursor += ch.len_utf8();
                }
                TextInputEvent::Changed
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cursor = 0;
                TextInputEvent::Changed
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cursor = self.value.len();
                TextInputEvent::Changed
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // delete word before cursor
                let end = self.cursor;
                let start = self.value[..end]
                    .rfind(|c: char| !c.is_whitespace())
                    .and_then(|i| self.value[..i].rfind(char::is_whitespace).map(|j| j + 1))
                    .unwrap_or(0);
                self.value.drain(start..end);
                self.cursor = start;
                TextInputEvent::Changed
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.value.drain(..self.cursor);
                self.cursor = 0;
                TextInputEvent::Changed
            }
            KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.value.truncate(self.cursor);
                TextInputEvent::Changed
            }
            KeyCode::Char(c) => {
                self.value.insert(self.cursor, c);
                self.cursor += c.len_utf8();
                TextInputEvent::Changed
            }
            _ => TextInputEvent::Ignored,
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn display(&self) -> String {
        let before = &self.value[..self.cursor];
        let after = &self.value[self.cursor..];
        format!("{before}|{after}")
    }
}
