use ratatui::widgets::ListState;

pub struct ScrollList {
    state: ListState,
    len: usize,
    wrap: bool,
}

impl ScrollList {
    pub fn new(len: usize) -> Self {
        let mut state = ListState::default();
        if len > 0 {
            state.select(Some(0));
        }
        Self { state, len, wrap: false }
    }

    pub fn wrap(&mut self, wrap: bool) {
        self.wrap = wrap;
    }

    pub fn set_len(&mut self, len: usize) {
        self.len = len;
        match self.state.selected() {
            Some(i) if i >= len => self.state.select(if len > 0 { Some(len - 1) } else { None }),
            None if len > 0 => self.state.select(Some(0)),
            _ => {}
        }
    }

    pub fn scroll_up(&mut self) {
        if self.len == 0 {
            return;
        }
        let i = self.state.selected().unwrap_or(0);
        let prev = if i == 0 {
            if self.wrap { self.len - 1 } else { 0 }
        } else {
            i - 1
        };
        self.state.select(Some(prev));
    }

    pub fn scroll_down(&mut self) {
        if self.len == 0 {
            return;
        }
        let i = self.state.selected().unwrap_or(0);
        let next = if i + 1 >= self.len {
            if self.wrap { 0 } else { self.len - 1 }
        } else {
            i + 1
        };
        self.state.select(Some(next));
    }

    pub fn select(&mut self, index: usize) {
        if self.len > 0 {
            self.state.select(Some(index.min(self.len - 1)));
        }
    }

    pub fn selected(&self) -> Option<usize> {
        self.state.selected()
    }

    pub fn state(&mut self) -> &mut ListState {
        &mut self.state
    }
}
