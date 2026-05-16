use hako::clipboard::clipboard;
use hako::{
    App, Event, Frame, KeyCode, ScrollList,
    image::{Picker, ResizeRequest, StatefulImage, StatefulProtocol, ThreadProtocol},
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Line, List, ListItem, Paragraph, Span},
};
use miette::Result;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::{
    State,
    images::{Image, ImageWithTags},
};

pub enum Action {
    Rename { index: usize, new_name: String },
}

pub struct KuraApp {
    #[allow(dead_code)]
    state: State,
    images: Vec<ImageWithTags>,
    scroll_list: ScrollList,

    should_quit: bool,

    rename_input: Option<String>,
    rename_error: Option<String>,

    image_protocol: ThreadProtocol,
    picker: Picker,
    /// Sends new protocols to the image loader task
    proto_tx: UnboundedSender<StatefulProtocol>,
    /// Receives fully loaded StatefulProtocols from the image loader task
    proto_rx: UnboundedReceiver<StatefulProtocol>,

    /// Receives encoded ResizeResponses from the background worker
    resize_rx: UnboundedReceiver<ResizeRequest>,
}

impl KuraApp {
    pub async fn new(state: State) -> Result<Self> {
        let images = Image::all(&state).await?;

        let scroll_list = ScrollList::new(images.len());

        let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());

        let (resize_tx, resize_rx) = unbounded_channel::<ResizeRequest>();

        // Channel: image loader task sends StatefulProtocol → proto_rx
        let (proto_tx, proto_rx) = unbounded_channel::<StatefulProtocol>();

        let image_protocol = ThreadProtocol::new(resize_tx, None);

        let mut app = Self {
            state,
            images,
            scroll_list,
            image_protocol,
            proto_rx,
            proto_tx,
            resize_rx,
            picker,
            should_quit: false,
            rename_input: None,
            rename_error: None,
        };

        app.load_selected_image();
        Ok(app)
    }

    fn load_selected_image(&mut self) {
        let Some(path) = self
            .scroll_list
            .selected()
            .and_then(|i| self.images.get(i))
            .map(|img| img.image.file_path.clone())
        else {
            return;
        };

        self.image_protocol.empty_protocol();

        let picker = self.picker.clone();
        let tx = self.proto_tx.clone();

        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                image::open(&path)
                    .ok()
                    .map(|dyn_img| picker.new_resize_protocol(dyn_img))
            })
            .await;

            if let Ok(Some(protocol)) = result {
                let _ = tx.send(protocol);
            }
        });
    }

    fn selected(&self) -> Option<&ImageWithTags> {
        self.scroll_list.selected().and_then(|i| self.images.get(i))
    }

    fn select_next(&mut self) {
        self.scroll_list.scroll_down();
        self.load_selected_image();
    }

    fn select_prev(&mut self) {
        self.scroll_list.scroll_up();
        self.load_selected_image();
    }

    fn copy_selected_to_clipboard(&self) {
        let Some(img) = self.selected() else { return };
        let Ok(ctx) = clipboard() else {
            return;
        };
        let _ = ctx.write_image(&img.image.file_path);
    }

    fn open_selected(&self) {
        let Some(img) = self.selected() else { return };
        let _ = std::process::Command::new("xdg-open")
            .arg(&img.image.file_path)
            .spawn();
    }
}

impl App for KuraApp {
    type Action = Action;

    fn should_quit(&self) -> bool {
        self.should_quit
    }

    fn handle_event(&mut self, event: &Event) -> Option<Action> {
        match event {
            Event::Key(key) => {
                if self.rename_input.is_some() {
                    match key.code {
                        KeyCode::Esc => {
                            self.rename_input = None;
                            self.rename_error = None;
                        }
                        KeyCode::Enter => {
                            if let (Some(buf), Some(index)) =
                                (self.rename_input.clone(), self.scroll_list.selected())
                            {
                                return Some(Action::Rename { index, new_name: buf });
                            }
                        }
                        KeyCode::Backspace => {
                            if let Some(buf) = &mut self.rename_input {
                                buf.pop();
                            }
                        }
                        KeyCode::Char(c) => {
                            if let Some(buf) = &mut self.rename_input {
                                buf.push(c);
                            }
                        }
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
                        KeyCode::Char('j') | KeyCode::Down => self.select_next(),
                        KeyCode::Char('k') | KeyCode::Up => self.select_prev(),
                        KeyCode::Enter => self.copy_selected_to_clipboard(),
                        KeyCode::Char('o') => self.open_selected(),
                        KeyCode::Char('r') => {
                            if let Some(img) = self.selected() {
                                self.rename_input = Some(img.image.name.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }
            Event::Tick => {
                self.rename_error = None;
                while let Ok(protocol) = self.proto_rx.try_recv() {
                    self.image_protocol.replace_protocol(protocol);
                }
                while let Ok(request) = self.resize_rx.try_recv() {
                    if let Ok(response) = request.resize_encode() {
                        self.image_protocol.update_resized_protocol(response);
                    }
                }
            }
            _ => {}
        }
        None
    }

    async fn on_action(&mut self, action: Action) {
        match action {
            Action::Rename { index, new_name } => {
                match self.images[index].image.rename(&self.state, &new_name).await {
                    Ok(_) => {
                        self.rename_input = None;
                        self.load_selected_image();
                    }
                    Err(e) => {
                        self.rename_error = Some(e.to_string());
                    }
                }
            }
        }
    }

    fn render(&mut self, frame: &mut Frame<'_>) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(frame.area());

        let items: Vec<ListItem> = self
            .images
            .iter()
            .map(|img| ListItem::new(img.image.name.clone()))
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Images"))
            .highlight_style(Style::default().fg(Color::Yellow))
            .highlight_symbol("> ");

        if let Some(buf) = &self.rename_input {
            let left_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(4)])
                .split(chunks[0]);

            frame.render_stateful_widget(list, left_chunks[0], self.scroll_list.state());

            let cursor_line = Line::from(format!("{buf}|"));
            let error_line = if let Some(err) = &self.rename_error {
                Line::from(Span::styled(err.as_str(), Style::default().fg(Color::Red)))
            } else {
                Line::from("")
            };

            let rename_box = Paragraph::new(vec![cursor_line, error_line])
                .block(Block::default().borders(Borders::ALL).title("Rename"));
            frame.render_widget(rename_box, left_chunks[1]);
        } else {
            frame.render_stateful_widget(list, chunks[0], self.scroll_list.state());
        }

        let is_animated = self
            .selected()
            .and_then(|img| img.image.mime_type())
            .map(|m| m.is_animated())
            .unwrap_or(false);

        let preview_title = if is_animated {
            "Preview  [animated — press o to open]"
        } else {
            "Preview"
        };

        let right_block = Block::default().borders(Borders::ALL).title(preview_title);
        let inner = right_block.inner(chunks[1]);
        frame.render_widget(right_block, chunks[1]);

        if self.image_protocol.protocol_type().is_some() {
            frame.render_stateful_widget(StatefulImage::new(), inner, &mut self.image_protocol);
        } else {
            let msg = if self.images.is_empty() {
                "No images found.".to_string()
            } else {
                "Loading...".to_string()
            };

            frame.render_widget(Paragraph::new(msg), inner);
        }
    }
}
