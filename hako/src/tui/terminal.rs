use std::time::Duration;

use crossterm::{
    cursor,
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event as CrosstermEvent, KeyEvent, KeyEventKind, MouseEvent,
    },
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::{FutureExt, StreamExt};
use ratatui::{backend::CrosstermBackend as Backend, layout::Rect};
use tokio::{
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

pub type IO = std::io::Stdout;

pub fn io() -> IO {
    std::io::stdout()
}

pub type Frame<'a> = ratatui::Frame<'a>;

pub enum Event {
    Error,
    FocusGained,
    FocusLost,
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    Render,
    Resize(u16, u16),
    Tick,
}

pub struct TerminalConfig {
    pub frame_rate: f64,
    pub tick_interval: Duration,
    pub mouse: bool,
    pub paste: bool,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            frame_rate: 60.0,
            tick_interval: Duration::from_millis(250),
            mouse: false,
            paste: false,
        }
    }
}

pub struct Terminal {
    terminal: ratatui::Terminal<Backend<IO>>,
    task: JoinHandle<()>,
    cancellation_token: CancellationToken,
    event_rx: UnboundedReceiver<Event>,
    event_tx: UnboundedSender<Event>,
    frame_rate: f64,
    tick_interval: Duration,
    mouse: bool,
    paste: bool,
}

impl Terminal {
    pub fn new(config: TerminalConfig) -> crate::error::Result<Self> {
        let terminal = ratatui::Terminal::new(Backend::new(io()))?;
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let cancellation_token = CancellationToken::new();
        let task = tokio::spawn(async {});

        Ok(Self {
            terminal,
            task,
            cancellation_token,
            event_rx,
            event_tx,
            frame_rate: config.frame_rate,
            tick_interval: config.tick_interval,
            mouse: config.mouse,
            paste: config.paste,
        })
    }

    pub fn draw<F>(&mut self, f: F) -> crate::error::Result<()>
    where
        F: FnOnce(&mut ratatui::Frame<'_>),
    {
        self.terminal.draw(f)?;
        Ok(())
    }

    fn start(&mut self) {
        let tick_delay = self.tick_interval;
        let render_delay = Duration::from_secs_f64(1.0 / self.frame_rate);
        self.cancel();
        self.cancellation_token = CancellationToken::new();
        let cancellation_token = self.cancellation_token.clone();
        let event_tx = self.event_tx.clone();

        self.task = tokio::spawn(async move {
            let mut reader = crossterm::event::EventStream::new();
            let mut tick_interval = tokio::time::interval(tick_delay);
            let mut render_interval = tokio::time::interval(render_delay);

            loop {
                let tick_delay = tick_interval.tick();
                let render_delay = render_interval.tick();
                let crossterm_event = reader.next().fuse();

                tokio::select! {
                    _ = cancellation_token.cancelled() => break,
                    maybe_event = crossterm_event => {
                        match maybe_event {
                            Some(Ok(evt)) => {
                                match evt {
                                    CrosstermEvent::Key(key) => {
                                        if key.kind == KeyEventKind::Press {
                                            let _ = event_tx.send(Event::Key(key));
                                        }
                                    }
                                    CrosstermEvent::Mouse(mouse) => {
                                        let _ = event_tx.send(Event::Mouse(mouse));
                                    }
                                    CrosstermEvent::Resize(x, y) => {
                                        let _ = event_tx.send(Event::Resize(x, y));
                                    }
                                    CrosstermEvent::FocusLost => {
                                        let _ = event_tx.send(Event::FocusLost);
                                    }
                                    CrosstermEvent::FocusGained => {
                                        let _ = event_tx.send(Event::FocusGained);
                                    }
                                    CrosstermEvent::Paste(s) => {
                                        let _ = event_tx.send(Event::Paste(s));
                                    }
                                }
                            }
                            Some(Err(_)) => {
                                let _ = event_tx.send(Event::Error);
                            }
                            None => {}
                        }
                    }
                    _ = tick_delay => {
                        let _ = event_tx.send(Event::Tick);
                    }
                    _ = render_delay => {
                        let _ = event_tx.send(Event::Render);
                    }
                }
            }
        });
    }

    fn stop(&self) {
        self.cancel();
        let mut counter = 0;
        while !self.task.is_finished() {
            std::thread::sleep(Duration::from_millis(1));
            counter += 1;
            if counter > 50 {
                self.task.abort();
                break;
            }
        }
    }

    pub fn enter(&mut self) -> crate::error::Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(io(), EnterAlternateScreen, cursor::Hide)?;
        if self.mouse {
            crossterm::execute!(io(), EnableMouseCapture)?;
        }
        if self.paste {
            crossterm::execute!(io(), EnableBracketedPaste)?;
        }
        self.start();
        Ok(())
    }

    pub fn exit(&mut self) -> crate::error::Result<()> {
        self.stop();
        if crossterm::terminal::is_raw_mode_enabled()? {
            self.terminal.flush()?;
            if self.paste {
                crossterm::execute!(io(), DisableBracketedPaste)?;
            }
            if self.mouse {
                crossterm::execute!(io(), DisableMouseCapture)?;
            }
            crossterm::execute!(io(), LeaveAlternateScreen, cursor::Show)?;
            crossterm::terminal::disable_raw_mode()?;
        }
        Ok(())
    }

    fn cancel(&self) {
        self.cancellation_token.cancel();
    }

    pub async fn next(&mut self) -> Option<Event> {
        let event = self.event_rx.recv().await?;
        if let Event::Resize(w, h) = &event {
            let _ = self.terminal.resize(Rect::new(0, 0, *w, *h));
        }
        Some(event)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.exit().unwrap();
    }
}
