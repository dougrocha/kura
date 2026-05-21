use hako::clipboard::clipboard;
use hako::{
    App, Event, Frame, KeyCode, ScrollList, TextInput, TextInputEvent,
    image::{Picker, ResizeRequest, StatefulImage, StatefulProtocol, ThreadProtocol},
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Line, List, ListItem, Paragraph, Span},
};
use miette::Result;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio_util::sync::CancellationToken;

use crate::{
    State,
    image_protocol_cache::ImageProtocolCache,
    images::{Image, ImageWithTags},
    tags::NewTag,
};

pub enum Action {
    Rename { index: usize, new_name: String },
    Tag { index: usize, tag: String },
    Untag { index: usize, tag: String },
}

pub struct KuraApp {
    #[allow(dead_code)]
    state: State,
    images: Vec<ImageWithTags>,
    scroll_list: ScrollList,

    should_quit: bool,

    rename_input: Option<TextInput>,
    rename_error: Option<String>,

    tag_input: Option<TextInput>,
    untag_input: Option<TextInput>,
    tag_error: Option<String>,

    image_protocol: ThreadProtocol,
    picker: Picker,
    /// Sends new protocols to the image loader task
    proto_tx: UnboundedSender<(usize, StatefulProtocol)>,
    /// Receives fully loaded StatefulProtocols from the image loader task
    proto_rx: UnboundedReceiver<(usize, StatefulProtocol)>,

    /// Receives encoded ResizeResponses from the background worker
    resize_rx: UnboundedReceiver<ResizeRequest>,

    protocol_cache: ImageProtocolCache,
    image_cancellation_token: CancellationToken,
}

impl KuraApp {
    pub async fn new(state: State) -> Result<Self> {
        let images = Image::all(&state).await?;

        let scroll_list = ScrollList::new(images.len());

        let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());

        let (resize_tx, resize_rx) = unbounded_channel::<ResizeRequest>();

        // Channel: image loader task sends StatefulProtocol → proto_rx
        let (proto_tx, proto_rx) = unbounded_channel::<(usize, StatefulProtocol)>();

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
            tag_input: None,
            untag_input: None,
            tag_error: None,
            protocol_cache: ImageProtocolCache::new(),
            image_cancellation_token: CancellationToken::new(),
        };

        app.load_selected_image();
        Ok(app)
    }

    fn load_selected_image(&mut self) {
        let Some(selected) = self.scroll_list.selected() else {
            return;
        };

        self.image_cancellation_token.cancel();
        self.image_cancellation_token = CancellationToken::new();

        self.image_protocol.empty_protocol();

        if let Some(protocol) = self.protocol_cache.take(selected) {
            self.image_protocol.replace_protocol(protocol);
        } else {
            self.spawn_image_load(selected);
        }

        let neighbors: Vec<usize> = [
            selected.wrapping_sub(2),
            selected.wrapping_sub(1),
            selected + 1,
            selected + 2,
        ]
        .into_iter()
        .filter(|&i| i < self.images.len() && !self.protocol_cache.contains(i))
        .collect();

        for index in neighbors {
            self.spawn_image_load(index);
        }
    }

    fn spawn_image_load(&self, index: usize) {
        let Some(img) = self.images.get(index) else {
            return;
        };

        let path = img.image.file_path.clone();
        let hash = img.image.hash.as_str().to_string();
        let picker = self.picker.clone();
        let tx = self.proto_tx.clone();
        let image_cache = self.state.image_cache.clone();
        let token = self.image_cancellation_token.clone();

        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                if token.is_cancelled() {
                    return None;
                }
                image_cache
                    .load_or_cache(&hash, &path)
                    .ok()
                    .and_then(|dyn_img| {
                        if token.is_cancelled() {
                            return None;
                        }
                        Some(picker.new_resize_protocol(dyn_img))
                    })
            })
            .await;

            if let Ok(Some(protocol)) = result {
                let _ = tx.send((index, protocol));
            }
        });
    }

    fn selected(&self) -> Option<&ImageWithTags> {
        self.scroll_list.selected().and_then(|i| self.images.get(i))
    }

    fn select_next(&mut self) {
        let before = self.scroll_list.selected();
        self.scroll_list.scroll_down();
        if self.scroll_list.selected() != before {
            self.load_selected_image();
        }
    }

    fn select_prev(&mut self) {
        let before = self.scroll_list.selected();
        self.scroll_list.scroll_up();
        if self.scroll_list.selected() != before {
            self.load_selected_image();
        }
    }

    fn copy_selected_to_clipboard(&self) {
        let Some(img) = self.selected() else { return };
        let Ok(ctx) = clipboard() else {
            return;
        };
        let _ = ctx.write_image(&img.image.file_path);
    }

    async fn reload_images(&mut self) {
        if let Ok(images) = Image::all(&self.state).await {
            let selected = self.scroll_list.selected();
            self.images = images;
            self.scroll_list = ScrollList::new(self.images.len());
            if let Some(i) = selected {
                let clamped = i.min(self.images.len().saturating_sub(1));
                self.scroll_list.select(clamped);
            }
        }
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
                if let Some(input) = &mut self.rename_input {
                    match input.handle_key(key) {
                        TextInputEvent::Submitted(val) => {
                            if let Some(index) = self.scroll_list.selected() {
                                return Some(Action::Rename {
                                    index,
                                    new_name: val,
                                });
                            }
                        }
                        TextInputEvent::Cancelled => {
                            self.rename_input = None;
                            self.rename_error = None;
                        }
                        _ => {}
                    }
                } else if let Some(input) = &mut self.tag_input {
                    match input.handle_key(key) {
                        TextInputEvent::Submitted(val) if !val.is_empty() => {
                            if let Some(index) = self.scroll_list.selected() {
                                return Some(Action::Tag { index, tag: val });
                            }
                        }
                        TextInputEvent::Cancelled => {
                            self.tag_input = None;
                            self.tag_error = None;
                        }
                        _ => {}
                    }
                } else if let Some(input) = &mut self.untag_input {
                    match input.handle_key(key) {
                        TextInputEvent::Submitted(val) if !val.is_empty() => {
                            if let Some(index) = self.scroll_list.selected() {
                                return Some(Action::Untag { index, tag: val });
                            }
                        }
                        TextInputEvent::Cancelled => {
                            self.untag_input = None;
                            self.tag_error = None;
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
                                self.rename_input = Some(TextInput::new(img.image.name.clone()));
                            }
                        }
                        KeyCode::Char('t') if self.selected().is_some() => {
                            self.tag_input = Some(TextInput::default());
                        }
                        KeyCode::Char('u') if self.selected().is_some() => {
                            self.untag_input = Some(TextInput::default());
                        }
                        _ => {}
                    }
                }
            }
            Event::Tick => {
                self.rename_error = None;
                let selected = self.scroll_list.selected().unwrap_or(0);
                while let Ok((index, protocol)) = self.proto_rx.try_recv() {
                    if index == selected {
                        self.image_protocol.replace_protocol(protocol);
                    } else {
                        self.protocol_cache.insert(index, protocol);
                    }
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
                match self.images[index]
                    .image
                    .rename(&self.state, &new_name)
                    .await
                {
                    Ok(_) => {
                        self.rename_input = None;
                        self.reload_images().await;
                        self.load_selected_image();
                    }
                    Err(e) => {
                        self.rename_error = Some(e.to_string());
                    }
                }
            }
            Action::Tag { index, tag } => {
                let hash = &self.images[index].image.hash;
                match NewTag::new(hash, &tag).insert(&self.state).await {
                    Ok(_) => {
                        self.tag_input = None;
                        self.tag_error = None;
                        self.reload_images().await;
                    }
                    Err(e) => {
                        self.tag_error = Some(e.to_string());
                    }
                }
            }
            Action::Untag { index, tag } => {
                let hash = self.images[index].image.hash.as_str().to_string();
                let result = sqlx::query!(
                    "DELETE FROM tags WHERE image_hash = ? AND tag = ?",
                    hash,
                    tag
                )
                .execute(&self.state.db_pool)
                .await;
                match result {
                    Ok(_) => {
                        self.untag_input = None;
                        self.tag_error = None;
                        self.reload_images().await;
                    }
                    Err(e) => {
                        self.tag_error = Some(e.to_string());
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

        let active_input: Option<(&TextInput, &Option<String>, &str)> =
            if let Some(input) = &self.rename_input {
                Some((input, &self.rename_error, "Rename"))
            } else if let Some(input) = &self.tag_input {
                Some((input, &self.tag_error, "Add tag"))
            } else if let Some(input) = &self.untag_input {
                Some((input, &self.tag_error, "Remove tag"))
            } else {
                None
            };

        if let Some((input, error, title)) = active_input {
            let left_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(4)])
                .split(chunks[0]);

            frame.render_stateful_widget(list, left_chunks[0], self.scroll_list.state());

            let cursor_line = Line::from(input.display());
            let error_line = if let Some(err) = error {
                Line::from(Span::styled(err.as_str(), Style::default().fg(Color::Red)))
            } else {
                Line::from("")
            };

            let input_box = Paragraph::new(vec![cursor_line, error_line])
                .block(Block::default().borders(Borders::ALL).title(title));
            frame.render_widget(input_box, left_chunks[1]);
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

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(3)])
            .split(chunks[1]);

        let right_block = Block::default().borders(Borders::ALL).title(preview_title);
        let inner = right_block.inner(right_chunks[0]);
        frame.render_widget(right_block, right_chunks[0]);

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

        let tags_text = if let Some(img) = self.selected() {
            if img.tags.is_empty() {
                Line::from(Span::styled(
                    "no tags",
                    Style::default().fg(Color::DarkGray),
                ))
            } else {
                let spans: Vec<Span> = img
                    .tags
                    .iter()
                    .flat_map(|t| {
                        [
                            Span::styled(t.tag.as_str(), Style::default().fg(Color::Yellow)),
                            Span::raw("  "),
                        ]
                    })
                    .collect();
                Line::from(spans)
            }
        } else {
            Line::from("")
        };

        let tags_block = Block::default()
            .borders(Borders::ALL)
            .title("Tags  [t] add  [u] remove");
        let tags_inner = tags_block.inner(right_chunks[1]);
        frame.render_widget(tags_block, right_chunks[1]);
        frame.render_widget(Paragraph::new(tags_text), tags_inner);
    }
}
