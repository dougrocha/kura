use crate::{TextInput, TextInputEvent};
use crossterm::event::KeyEvent;

pub enum FormFieldEvent {
    /// Field value submitted (Enter pressed)
    Submitted(String),
    /// User cancelled out of form (Esc pressed)
    Cancelled,
    /// Focus moved to next field (Tab pressed)
    Next,
    /// Focus moved to previous field (Shift+Tab pressed)
    Previous,
    /// Field lost focus and should be validated
    Blurred(String),
    /// Field value changed internally (not useful for form but kept for compatibility)
    Changed,
}

pub struct FormField {
    label: String,
    hint: Option<String>,
    input: TextInput,
    validator: Option<Box<dyn Fn(&str) -> Result<(), String>>>,
    error: Option<String>,
    placeholder: String,
    required: bool,
}

impl FormField {
    pub fn new(label: impl Into<String>, placeholder: impl Into<String>, required: bool) -> Self {
        Self {
            label: label.into(),
            hint: None,
            input: TextInput::default(),
            validator: None,
            error: None,
            placeholder: placeholder.into(),
            required,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    pub fn with_validator<F>(mut self, validator: F) -> Self
    where
        F: Fn(&str) -> Result<(), String> + 'static,
    {
        self.validator = Some(Box::new(validator));
        self
    }

    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.input = TextInput::new(value.into());
        self
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn value(&self) -> &str {
        self.input.value()
    }

    pub fn set_value(&mut self, value: impl Into<String>) {
        self.input = TextInput::new(value.into());
        self.error = None;
    }

    pub fn set_placeholder(&mut self, placeholder: impl Into<String>) {
        self.placeholder = placeholder.into();
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn validate(&mut self) -> Result<(), String> {
        let value = self.input.value();

        if self.required && value.is_empty() {
            let err = format!("{} is required", self.label);
            self.error = Some(err.clone());
            return Err(err);
        }

        if let Some(validator) = &self.validator {
            match validator(value) {
                Ok(_) => {
                    self.error = None;
                    Ok(())
                }
                Err(e) => {
                    self.error = Some(e.clone());
                    Err(e)
                }
            }
        } else {
            self.error = None;
            Ok(())
        }
    }

    pub fn clear_error(&mut self) {
        self.error = None;
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> FormFieldEvent {
        use crossterm::event::KeyCode;

        // Handle Tab/Shift+Tab for navigation before passing to TextInput
        match key.code {
            KeyCode::BackTab => {
                return FormFieldEvent::Previous;
            }
            KeyCode::Tab => {
                // Validate before moving to next field
                let value = self.input.value().to_string();
                if self.validate().is_err() {
                    return FormFieldEvent::Changed;
                }
                return FormFieldEvent::Blurred(value);
            }
            _ => {}
        }

        match self.input.handle_key(key) {
            TextInputEvent::Submitted(val) => FormFieldEvent::Submitted(val),
            TextInputEvent::Cancelled => FormFieldEvent::Cancelled,
            TextInputEvent::Changed => FormFieldEvent::Changed,
            TextInputEvent::Ignored => FormFieldEvent::Changed,
        }
    }

    pub fn display(&self) -> String {
        self.input.display()
    }

    pub fn placeholder(&self) -> &str {
        &self.placeholder
    }
}
