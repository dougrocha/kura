use hako::clipboard::ClipboardContext;
use hako::{
    App, Event, Frame, KeyCode,
    image::{Picker, ResizeRequest, StatefulImage, StatefulProtocol, ThreadProtocol},
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use miette::Result;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::{
    State,
    images::{Image, ImageWithTags},
};

pub struct KuraApp {
    #[allow(dead_code)]
    state: State,
    images: Vec<ImageWithTags>,
    list_state: ListState,

    should_quit: bool,

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

        let mut list_state = ListState::default();
        if !images.is_empty() {
            list_state.select(Some(0));
        }

        let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());

        let (resize_tx, resize_rx) = unbounded_channel::<ResizeRequest>();

        // Channel: image loader task sends StatefulProtocol → proto_rx
        let (proto_tx, proto_rx) = unbounded_channel::<StatefulProtocol>();

        let image_protocol = ThreadProtocol::new(resize_tx, None);

        let mut app = Self {
            state,
            images,
            list_state,
            image_protocol,
            proto_rx,
            proto_tx,
            resize_rx,
            picker,
            should_quit: false,
        };

        app.load_selected_image();
        Ok(app)
    }

    fn load_selected_image(&mut self) {
        let Some(path) = self
            .list_state
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
        self.list_state.selected().and_then(|i| self.images.get(i))
    }

    fn select_next(&mut self) {
        if self.images.is_empty() {
            return;
        }
        let next = self
            .list_state
            .selected()
            .map_or(0, |i| if i + 1 >= self.images.len() { 0 } else { i + 1 });
        self.list_state.select(Some(next));
        self.load_selected_image();
    }

    fn select_prev(&mut self) {
        if self.images.is_empty() {
            return;
        }

        let prev = self
            .list_state
            .selected()
            .map_or(0, |i| if i == 0 { self.images.len() - 1 } else { i - 1 });
        self.list_state.select(Some(prev));
        self.load_selected_image();
    }

    fn copy_selected_to_clipboard(&self) {
        let Some(img) = self.selected() else { return };
        let Ok(ctx) = ClipboardContext::new() else {
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
    fn should_quit(&self) -> bool {
        self.should_quit
    }

    fn handle_event(&mut self, event: &Event) {
        match event {
            Event::Key(key) => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
                KeyCode::Char('j') | KeyCode::Down => self.select_next(),
                KeyCode::Char('k') | KeyCode::Up => self.select_prev(),
                KeyCode::Enter => self.copy_selected_to_clipboard(),
                KeyCode::Char('o') => self.open_selected(),
                _ => {}
            },
            Event::Tick => {
                // Pick up freshly loaded protocols
                while let Ok(protocol) = self.proto_rx.try_recv() {
                    self.image_protocol.replace_protocol(protocol);
                }

                // Pick up completed resize/encode responses
                while let Ok(request) = self.resize_rx.try_recv() {
                    if let Ok(response) = request.resize_encode() {
                        self.image_protocol.update_resized_protocol(response);
                    }
                }
            }
            _ => {}
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

        frame.render_stateful_widget(list, chunks[0], &mut self.list_state);

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
