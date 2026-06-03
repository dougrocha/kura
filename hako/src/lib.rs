pub mod clipboard;
pub mod component;
pub mod error;
pub mod form;
pub mod form_field;
pub mod mime_type;
pub mod scroll_list;
pub mod terminal;
pub mod text_input;
pub mod url;

use std::future::Future;

pub use component::{Component, Components, Context, EventPropagation, Position};
pub use error::{Error, Result};
pub use form::{Form, FormEvent};
pub use form_field::FormField;
pub use ratatui::widgets::ListState;
pub use scroll_list::ScrollList;
pub use terminal::{Event, Frame, Terminal, TerminalConfig};
pub use text_input::{TextInput, TextInputEvent};

pub use crossterm::event::KeyCode;

pub mod layout {
    pub use ratatui::layout::{Constraint, Direction, Layout};
}

pub mod style {
    pub use ratatui::style::{Color, Style};
}

pub mod widgets {
    pub use ratatui::text::{Line, Span};
    pub use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
}

pub mod image {
    pub use ratatui_image::{
        Resize, StatefulImage,
        picker::Picker,
        protocol::StatefulProtocol,
        thread::{ResizeRequest, ResizeResponse, ThreadProtocol},
    };
}

/// A ready-made event loop. Implement this trait on your app state, then call
/// `hako::run(state)` to start the TUI.
pub trait App: Sized {
    type Action;

    fn should_quit(&self) -> bool;

    fn handle_event(&mut self, event: &Event) -> Option<Self::Action>;

    fn on_action(&mut self, action: Self::Action) -> impl Future<Output = ()>;

    fn render(&mut self, frame: &mut Frame<'_>);
}

pub async fn run<A: App>(mut app: A) -> Result<()> {
    let mut terminal = Terminal::new(TerminalConfig::default())?;
    terminal.enter()?;

    loop {
        if app.should_quit() {
            break;
        }

        match terminal.next().await {
            Some(Event::Render) => {
                terminal.draw(|frame| app.render(frame))?;
            }
            Some(event) => {
                if let Some(action) = app.handle_event(&event) {
                    app.on_action(action).await;
                }
            }
            None => break,
        }
    }

    terminal.exit()?;
    Ok(())
}
