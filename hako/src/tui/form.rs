use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::widgets::Clear;
use crate::{Frame, FormField};
use crate::form_field::FormFieldEvent;
use crate::widgets::{Block, Borders, Line, Paragraph, Span};
use crate::style::{Color, Style};

pub enum FormEvent {
    /// Form submitted with all field values
    Submitted(Vec<String>),
    /// Form cancelled
    Cancelled,
    /// Field blurred (Tab pressed) - for async validation or auto-fill
    FieldBlurred { field_index: usize, value: String },
}

pub struct Form {
    title: String,
    fields: Vec<FormField>,
    focused_field: usize,
    form_error: Option<String>,
}

impl Form {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            fields: Vec::new(),
            focused_field: 0,
            form_error: None,
        }
    }

    pub fn add_field(mut self, field: FormField) -> Self {
        self.fields.push(field);
        self
    }

    pub fn focused_field(&self) -> usize {
        self.focused_field
    }

    pub fn set_field_value(&mut self, index: usize, value: impl Into<String>) {
        if let Some(field) = self.fields.get_mut(index) {
            field.set_value(value);
        }
    }

    pub fn set_field_placeholder(&mut self, index: usize, placeholder: impl Into<String>) {
        if let Some(field) = self.fields.get_mut(index) {
            field.set_placeholder(placeholder);
        }
    }

    pub fn field_value(&self, index: usize) -> Option<&str> {
        self.fields.get(index).map(|f| f.value())
    }

    pub fn set_form_error(&mut self, error: Option<String>) {
        self.form_error = error;
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> Option<FormEvent> {
        if self.fields.is_empty() {
            return None;
        }

        let event = self.fields[self.focused_field].handle_key(key);

        match event {
            FormFieldEvent::Submitted(_) => {
                // Validate all fields before submitting
                for field in &mut self.fields {
                    if field.validate().is_err() {
                        return None;
                    }
                }
                let values = self.fields.iter().map(|f| f.value().to_string()).collect();
                Some(FormEvent::Submitted(values))
            }
            FormFieldEvent::Cancelled => Some(FormEvent::Cancelled),
            FormFieldEvent::Next => {
                self.focused_field = (self.focused_field + 1) % self.fields.len();
                None
            }
            FormFieldEvent::Previous => {
                self.focused_field = if self.focused_field == 0 {
                    self.fields.len() - 1
                } else {
                    self.focused_field - 1
                };
                None
            }
            FormFieldEvent::Blurred(val) => {
                let index = self.focused_field;
                self.focused_field = (self.focused_field + 1) % self.fields.len();
                Some(FormEvent::FieldBlurred {
                    field_index: index,
                    value: val,
                })
            }
            FormFieldEvent::Changed => None,
        }
    }

    pub fn render(&self, frame: &mut Frame<'_>) {
        let modal_area = calculate_modal_area(frame.area(), 75, (self.fields.len() as u16) * 3 + 6);

        frame.render_widget(Clear, modal_area);

        let block = Block::default()
            .title(self.title.as_str())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::White));

        let inner = block.inner(modal_area);

        frame.render_widget(block, modal_area);

        let mut y = inner.y;

        // Render form-level error if present
        if let Some(error) = &self.form_error {
            let error_line = Line::from(Span::styled(
                format!("Error: {}", error),
                Style::default().fg(Color::Red),
            ));
            let error_area = Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            };
            frame.render_widget(Paragraph::new(error_line), error_area);
            y += 2;
        }

        // Render each field
        for (i, field) in self.fields.iter().enumerate() {
            if y >= inner.y + inner.height.saturating_sub(2) {
                break;
            }

            let is_focused = i == self.focused_field;

            // Label line with focus indicator and optional hint
            let prefix = if is_focused { "> " } else { "  " };
            let label_base = format!("{}{}: ", prefix, field.label());
            let label_line = if let Some(hint) = field.hint() {
                Line::from(vec![
                    Span::styled(label_base, Style::default().fg(Color::White)),
                    Span::styled(hint.to_string(), Style::default().fg(Color::DarkGray)),
                ])
            } else {
                Line::from(Span::styled(label_base, Style::default().fg(Color::White)))
            };
            let label_area = Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            };
            frame.render_widget(Paragraph::new(label_line), label_area);
            y += 1;

            // Input line with placeholder or value
            let is_placeholder = field.value().is_empty() && !field.placeholder().is_empty();
            let input_text = if is_focused && is_placeholder {
                format!("  {}", field.placeholder())
            } else if is_focused {
                format!("  {}", field.display())
            } else if is_placeholder {
                format!("  {}", field.placeholder())
            } else {
                format!("  {}", field.value())
            };
            let input_color = if field.error().is_some() {
                Color::Red
            } else if is_placeholder {
                Color::DarkGray
            } else if is_focused {
                Color::Yellow
            } else {
                Color::White
            };

            let input_line = Line::from(Span::styled(
                input_text,
                Style::default().fg(input_color)
            ));
            let input_area = Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            };
            frame.render_widget(Paragraph::new(input_line), input_area);
            y += 1;

            // Error line if present
            if let Some(error) = field.error() {
                let error_line = Line::from(Span::styled(
                    format!("  {}", error),
                    Style::default().fg(Color::Red),
                ));
                let error_area = Rect {
                    x: inner.x,
                    y,
                    width: inner.width,
                    height: 1,
                };
                frame.render_widget(Paragraph::new(error_line), error_area);
                y += 1;
            }
        }

        // Help text at bottom
        if y < inner.y + inner.height {
            y = inner.y + inner.height.saturating_sub(1);
            let help_line = Line::from(Span::styled(
                "Tab/Shift+Tab: navigate  |  Enter: submit  |  Esc: cancel",
                Style::default().fg(Color::DarkGray),
            ));
            let help_area = Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            };
            frame.render_widget(Paragraph::new(help_line), help_area);
        }
    }
}

/// Calculate centered modal area given terminal area and desired dimensions
fn calculate_modal_area(terminal_area: Rect, width_percent: u16, desired_height: u16) -> Rect {
    let width = (terminal_area.width * width_percent / 100).max(40).min(terminal_area.width);
    let height = desired_height.min(terminal_area.height);
    let x = (terminal_area.width.saturating_sub(width)) / 2;
    let y = (terminal_area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
