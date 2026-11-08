use ratatui::layout::Rect;

use crate::terminal::{Event, Frame};

pub struct Position {
    pub x: usize,
    pub y: usize,
}

/// Passed to every component on each event and render call, holding a mutable
/// reference to your application state.
pub struct Context<'a, S> {
    pub state: &'a mut S,
}

type EventCallback<S> = Box<dyn FnOnce(&mut Components<S>, &mut Context<S>)>;

pub enum EventPropagation<S> {
    /// Event was not consumed; optionally schedule a callback.
    Ignore(Option<EventCallback<S>>),
    /// Event was consumed; stop propagation; optionally schedule a callback.
    Consume(Option<EventCallback<S>>),
}

pub trait Component<S> {
    fn handle_events(
        &mut self,
        _event: &Event,
        _context: &mut Context<S>,
    ) -> EventPropagation<S> {
        EventPropagation::Ignore(None)
    }

    fn cursor(&self, _area: Rect, _context: &mut S) -> Option<Position> {
        None
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect, context: &mut Context<S>);
}

pub struct Components<S> {
    components: Vec<Box<dyn Component<S>>>,
    area: Rect,
}

impl<S> Components<S> {
    pub fn new(area: Rect) -> Self {
        Self {
            components: vec![],
            area,
        }
    }

    pub fn area(&self) -> Rect {
        self.area
    }

    pub fn resize(&mut self, area: Rect) {
        self.area = area;
    }

    pub fn push(&mut self, component: Box<dyn Component<S>>) {
        self.components.push(component);
    }

    pub fn pop(&mut self) {
        self.components.pop();
    }

    pub fn cursor(&self, context: &mut Context<S>) -> Option<Position> {
        for component in self.components.iter().rev() {
            if let Some(cursor) = component.cursor(self.area, context.state) {
                return Some(cursor);
            }
        }
        None
    }

    /// Returns true if any component consumed the event.
    pub fn handle_events(&mut self, event: &Event, context: &mut Context<S>) -> bool {
        let mut callbacks: Vec<EventCallback<S>> = vec![];
        let mut consumed = false;

        for component in self.components.iter_mut().rev() {
            match component.handle_events(event, context) {
                EventPropagation::Ignore(Some(cb)) => callbacks.push(cb),
                EventPropagation::Ignore(None) => {}
                EventPropagation::Consume(Some(cb)) => {
                    callbacks.push(cb);
                    consumed = true;
                    break;
                }
                EventPropagation::Consume(None) => {
                    consumed = true;
                    break;
                }
            }
        }

        for cb in callbacks {
            cb(self, context);
        }

        consumed
    }

    pub fn render(&mut self, frame: &mut Frame<'_>, context: &mut Context<S>) {
        for component in &mut self.components {
            component.render(frame, self.area, context);
        }
        if let Some(pos) = self.cursor(context) {
            frame.set_cursor_position((pos.x as u16, pos.y as u16));
        }
    }
}
