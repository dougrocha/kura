use hako::clipboard::clipboard;
use hako::{
    App, Event, Frame, KeyCode, ScrollList, TextInput, TextInputEvent,
    image::{Picker, ResizeRequest, StatefulImage, StatefulProtocol, ThreadProtocol},
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Line, List, ListItem, Paragraph, Span},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio_util::sync::CancellationToken;

use kura_core::{
    ChangeSeq, Coverage, Hit, Image, ImageHash, ImageName, Library, Outcome, Reading, SearchQuery, Snippet, Tag,
    ThumbSize,
};

use crate::image_protocol_cache::ImageProtocolCache;

pub enum Action {
    Rename {
        hash: ImageHash,
        new_name: String,
    },
    Tag {
        hash: ImageHash,
        tag: String,
    },
    Untag {
        hash: ImageHash,
        tag: String,
    },
    /// Run the search again. A new query selects the best match; otherwise the
    /// selected image stays selected.
    Search {
        new_query: bool,
    },
    /// Every tick: search again if someone else (the Mac app, the CLI) wrote.
    Poll,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Browse,
    /// Like `Browse`, but Enter also quits after copying.
    Pick,
}

pub struct KuraApp {
    lib: Library,
    mode: Mode,
    /// `pick.close_after_copy`: Enter in the picker exits.
    close_after_copy: bool,
    /// What the search matched, best first. An empty query matches everything.
    hits: Vec<Hit>,
    coverage: Coverage,
    /// The change-log position `hits` reflects.
    seq: ChangeSeq,
    search_error: Option<String>,
    scroll_list: ScrollList,

    search_input: Option<TextInput>,
    search_query: String,

    should_quit: bool,

    rename_input: Option<TextInput>,
    rename_error: Option<String>,

    tag_input: Option<TextInput>,
    untag_input: Option<TextInput>,
    tag_error: Option<String>,

    image_protocol: ThreadProtocol,
    picker: Picker,
    /// Sends new protocols to the image loader task
    proto_tx: UnboundedSender<(String, StatefulProtocol)>,
    /// Receives fully loaded StatefulProtocols from the image loader task
    proto_rx: UnboundedReceiver<(String, StatefulProtocol)>,

    /// Receives encoded ResizeResponses from the background worker
    resize_rx: UnboundedReceiver<ResizeRequest>,

    protocol_cache: ImageProtocolCache,
    image_cancellation_token: CancellationToken,
}

impl KuraApp {
    pub async fn new(lib: Library, mode: Mode, close_after_copy: bool) -> kura_core::Result<Self> {
        let results = lib.search(&SearchQuery::parse("")).await?;
        let scroll_list = ScrollList::new(results.hits.len());

        let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());

        let (resize_tx, resize_rx) = unbounded_channel::<ResizeRequest>();

        // Channel: image loader task sends StatefulProtocol → proto_rx
        let (proto_tx, proto_rx) = unbounded_channel::<(String, StatefulProtocol)>();

        let image_protocol = ThreadProtocol::new(resize_tx, None);

        let mut app = Self {
            lib,
            mode,
            close_after_copy,
            hits: results.hits,
            coverage: results.coverage,
            seq: results.seq,
            search_error: None,
            scroll_list,
            search_input: None,
            search_query: String::new(),
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
        self.image_cancellation_token.cancel();
        self.image_cancellation_token = CancellationToken::new();

        self.image_protocol.empty_protocol();

        let Some(position) = self.scroll_list.selected() else {
            return;
        };
        let selected_hash = self.hits[position].image().hash().clone();
        if let Some(protocol) = self.protocol_cache.take(selected_hash.as_str()) {
            self.image_protocol.replace_protocol(protocol);
        } else {
            self.spawn_image_load(selected_hash);
        }

        let neighbors: Vec<ImageHash> =
            [position.wrapping_sub(2), position.wrapping_sub(1), position + 1, position + 2]
                .into_iter()
                .filter_map(|p| self.hits.get(p))
                .map(|hit| hit.image().hash())
                .filter(|hash| !self.protocol_cache.contains(hash.as_str()))
                .cloned()
                .collect();

        for hash in neighbors {
            self.spawn_image_load(hash);
        }
    }

    fn spawn_image_load(&self, hash: ImageHash) {
        let lib = self.lib.clone();
        let picker = self.picker.clone();
        let tx = self.proto_tx.clone();
        let token = self.image_cancellation_token.clone();

        tokio::spawn(async move {
            // kura-core makes (or reuses) the thumbnail; decoding it for the
            // terminal stays here.
            let Ok(thumb) = lib.thumbnail(&hash, ThumbSize::Preview).await else {
                return;
            };
            let path = thumb.path().to_path_buf();
            let result = tokio::task::spawn_blocking(move || {
                if token.is_cancelled() {
                    return None;
                }
                let dyn_img = image::open(&path).ok()?;
                if token.is_cancelled() {
                    return None;
                }
                Some(picker.new_resize_protocol(dyn_img))
            })
            .await;

            if let Ok(Some(protocol)) = result {
                let _ = tx.send((hash.to_string(), protocol));
            }
        });
    }

    fn selected(&self) -> Option<&Image> {
        self.scroll_list.selected().and_then(|position| self.hits.get(position)).map(Hit::image)
    }

    fn selected_hash(&self) -> Option<ImageHash> {
        self.selected().map(|image| image.hash().clone())
    }

    /// Searches again. A new query selects the best match; a refresh keeps the
    /// selected image selected (by hash, so rows moving doesn't shift it).
    async fn apply_search(&mut self, new_query: bool) {
        let before = self.selected_hash();
        match self.lib.search(&SearchQuery::parse(&self.search_query)).await {
            Ok(results) => {
                self.search_error = None;
                self.seq = results.seq;
                self.coverage = results.coverage;
                self.hits = results.hits;
            }
            Err(e) => {
                self.search_error = Some(e.to_string());
                return;
            }
        }
        self.scroll_list = ScrollList::new(self.hits.len());
        if let Some(position) = before
            .as_ref()
            .filter(|_| !new_query)
            .and_then(|hash| self.hits.iter().position(|hit| hit.image().hash() == hash))
        {
            self.scroll_list.select(position);
        }
        if self.selected_hash() != before {
            self.load_selected_image();
        }
    }

    /// Searching again when the library changed under us: the Mac app wrote a
    /// caption, or the CLI added an image.
    async fn refresh_if_changed(&mut self) {
        if self.lib.head().await.is_ok_and(|head| head != self.seq) {
            self.apply_search(false).await;
        }
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
        let _ = ctx.write_image(&self.lib.path_of(img));
    }

    fn open_selected(&self) {
        let Some(img) = self.selected() else { return };
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        let _ = std::process::Command::new(opener).arg(self.lib.path_of(img)).spawn();
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
                // Handle existing text inputs
                if let Some(input) = &mut self.search_input {
                    match input.handle_key(key) {
                        TextInputEvent::Submitted(_) => self.search_input = None,
                        TextInputEvent::Cancelled => {
                            self.search_input = None;
                            self.search_query.clear();
                            return Some(Action::Search { new_query: false });
                        }
                        TextInputEvent::Changed => {
                            self.search_query = input.value().to_string();
                            return Some(Action::Search { new_query: true });
                        }
                        TextInputEvent::Ignored => match key.code {
                            KeyCode::Down => self.select_next(),
                            KeyCode::Up => self.select_prev(),
                            _ => {}
                        },
                    }
                } else if let Some(input) = &mut self.rename_input {
                    match input.handle_key(key) {
                        TextInputEvent::Submitted(val) => {
                            if let Some(hash) = self.selected_hash() {
                                return Some(Action::Rename { hash, new_name: val });
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
                            if let Some(hash) = self.selected_hash() {
                                return Some(Action::Tag { hash, tag: val });
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
                            if let Some(hash) = self.selected_hash() {
                                return Some(Action::Untag { hash, tag: val });
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
                        KeyCode::Esc if !self.search_query.is_empty() => {
                            self.search_query.clear();
                            return Some(Action::Search { new_query: false });
                        }
                        KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
                        KeyCode::Char('/') => {
                            self.search_input = Some(TextInput::new(self.search_query.clone()));
                        }
                        KeyCode::Char('j') | KeyCode::Down => self.select_next(),
                        KeyCode::Char('k') | KeyCode::Up => self.select_prev(),
                        KeyCode::Enter => {
                            self.copy_selected_to_clipboard();
                            if self.mode == Mode::Pick && self.close_after_copy {
                                self.should_quit = true;
                            }
                        }
                        KeyCode::Char('o') => self.open_selected(),
                        KeyCode::Char('r') => {
                            if let Some(img) = self.selected() {
                                self.rename_input = Some(TextInput::new(img.name().to_string()));
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
                let selected_hash = self.selected().map(|img| img.hash().to_string());
                while let Ok((hash, protocol)) = self.proto_rx.try_recv() {
                    if selected_hash.as_deref() == Some(hash.as_str()) {
                        self.image_protocol.replace_protocol(protocol);
                    } else {
                        self.protocol_cache.insert(hash, protocol);
                    }
                }
                while let Ok(request) = self.resize_rx.try_recv() {
                    if let Ok(response) = request.resize_encode() {
                        self.image_protocol.update_resized_protocol(response);
                    }
                }
                return Some(Action::Poll);
            }
            _ => {}
        }
        None
    }

    async fn on_action(&mut self, action: Action) {
        match action {
            Action::Search { new_query } => self.apply_search(new_query).await,
            Action::Poll => self.refresh_if_changed().await,
            Action::Rename { hash, new_name } => {
                let result = match ImageName::parse(&new_name) {
                    Ok(name) => self.lib.rename(&hash, name).await,
                    Err(e) => Err(e.into()),
                };
                match result {
                    Ok(_) => {
                        self.rename_input = None;
                        self.apply_search(false).await;
                    }
                    Err(e) => {
                        self.rename_error = Some(e.to_string());
                    }
                }
            }
            Action::Tag { hash, tag } => {
                let result = match Tag::parse(&tag) {
                    Ok(tag) => self.lib.tag(&hash, &[tag]).await,
                    Err(e) => Err(e.into()),
                };
                match result {
                    Ok(_) => {
                        self.tag_input = None;
                        self.tag_error = None;
                        self.apply_search(false).await;
                    }
                    Err(e) => {
                        self.tag_error = Some(e.to_string());
                    }
                }
            }
            Action::Untag { hash, tag } => {
                let result = match Tag::parse(&tag) {
                    Ok(tag) => self.lib.untag(&hash, &tag).await,
                    Err(e) => Err(e.into()),
                };
                match result {
                    Ok(_) => {
                        self.untag_input = None;
                        self.tag_error = None;
                        self.apply_search(false).await;
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

        let items: Vec<ListItem> = self.hits.iter().map(row).collect();

        let list_title = if self.search_query.is_empty() {
            "Images  [/] search  [r] rename".to_string()
        } else {
            format!("Images {}/{}  /{}  [Esc] clear", self.hits.len(), self.coverage.images, self.search_query)
        };

        let dim = Style::default().fg(Color::DarkGray);
        let footer = if let Some(error) = &self.search_error {
            Line::from(Span::styled(error.as_str(), Style::default().fg(Color::Red)))
        } else if self.coverage.unread > 0 {
            Line::from(Span::styled(format!("{} not read yet", self.coverage.unread), dim))
        } else {
            Line::from("")
        };

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(list_title).title_bottom(footer))
            .highlight_style(Style::default().fg(Color::Yellow))
            .highlight_symbol("> ");

        let active_input: Option<(&TextInput, Option<&str>, &str)> = if let Some(input) = &self.search_input {
            Some((input, None, "Search  name, tags, text, #tag"))
        } else if let Some(input) = &self.rename_input {
            Some((input, self.rename_error.as_deref(), "Rename"))
        } else if let Some(input) = &self.tag_input {
            Some((input, self.tag_error.as_deref(), "Add tag"))
        } else if let Some(input) = &self.untag_input {
            Some((input, self.tag_error.as_deref(), "Remove tag"))
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
                Line::from(Span::styled(err, Style::default().fg(Color::Red)))
            } else {
                Line::from("")
            };

            let input_box = Paragraph::new(vec![cursor_line, error_line])
                .block(Block::default().borders(Borders::ALL).title(title));
            frame.render_widget(input_box, left_chunks[1]);
        } else {
            frame.render_stateful_widget(list, chunks[0], self.scroll_list.state());
        }

        let is_animated = self.selected().and_then(|img| img.media_type()).map(|m| m.is_animated()).unwrap_or(false);

        let preview_title = if is_animated { "Preview  [animated — press o to open]" } else { "Preview" };

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
            let msg = if self.coverage.images == 0 {
                "No images found.".to_string()
            } else if self.hits.is_empty() {
                "No matches.".to_string()
            } else {
                "Loading...".to_string()
            };

            frame.render_widget(Paragraph::new(msg), inner);
        }

        let tags_text = if let Some(img) = self.selected() {
            let mut spans: Vec<Span> = if img.tags().is_empty() {
                vec![Span::styled("no tags", dim)]
            } else {
                img.tags()
                    .iter()
                    .flat_map(|t| [Span::styled(t.as_str(), Style::default().fg(Color::Yellow)), Span::raw("  ")])
                    .collect()
            };
            if let Some(caption) = caption_text(img) {
                spans.push(Span::styled(format!("  {caption}"), dim));
            }
            Line::from(spans)
        } else {
            Line::from("")
        };

        let tags_block =
            Block::default().borders(Borders::ALL).title("Tags  [Enter] copy  [o] open  [t] add  [u] remove");
        let tags_inner = tags_block.inner(right_chunks[1]);
        frame.render_widget(tags_block, right_chunks[1]);
        frame.render_widget(Paragraph::new(tags_text), tags_inner);
    }
}

/// A list row: the name, and for a caption hit a dim `· …snippet…` with the
/// matched words in white.
fn row(hit: &Hit) -> ListItem<'static> {
    let name = hit.image().name().to_string();
    match hit.snippet() {
        None => ListItem::new(name),
        Some(snippet) => {
            let dim = Style::default().fg(Color::DarkGray);
            let mut spans = vec![Span::raw(name), Span::styled(" · ", dim)];
            spans.extend(snippet_spans(snippet));
            ListItem::new(Line::from(spans))
        }
    }
}

fn snippet_spans(snippet: &Snippet) -> Vec<Span<'static>> {
    let dim = Style::default().fg(Color::DarkGray);
    let matched = Style::default().fg(Color::White);
    let cut = |show: bool| show.then(|| Span::styled("…", dim));

    let mut spans: Vec<Span> = cut(snippet.leading_cut).into_iter().collect();
    spans.extend(snippet.segments.iter().map(|s| Span::styled(s.text.clone(), if s.matched { matched } else { dim })));
    spans.extend(cut(snippet.trailing_cut));
    spans
}

/// The caption to show under the preview: the text, marked with a pencil when a
/// person wrote it. Nothing when it's unread or has no text.
fn caption_text(image: &Image) -> Option<String> {
    let reading = image.caption()?;
    let Outcome::Text(text) = reading.outcome() else { return None };
    Some(match reading {
        Reading::Manual { .. } => format!("✎ {text}"),
        Reading::Ocr { .. } => text.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use kura_core::{CaptionEdit, CaptionText, LibraryPaths, NewName, Segment, Source};

    use super::*;

    fn png(seed: u8) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbImage::from_pixel(4, 4, image::Rgb([seed, 0, 0]))
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    async fn add(lib: &Library, seed: u8, name: &str) -> ImageHash {
        let name = ImageName::parse(name).unwrap();
        let added = lib.add(Source::bytes(png(seed), None), NewName::Exactly(name), &[], None).await.unwrap();
        added.image().hash().clone()
    }

    fn caption(text: &str) -> CaptionEdit {
        CaptionEdit::Manual(CaptionText::normalize(text))
    }

    fn names(app: &KuraApp) -> Vec<&str> {
        app.hits.iter().map(|hit| hit.image().name().as_str()).collect()
    }

    #[tokio::test]
    async fn search_keeps_selection_by_hash_and_follows_other_writers() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(LibraryPaths::under(tmp.path())).await.unwrap();
        let office = add(&lib, 1, "office").await;
        let moon = add(&lib, 2, "moon baby").await;
        add(&lib, 3, "plain").await;
        let _ = lib.set_caption(&office, caption("WHEN THE BOSS WALKS IN")).await.unwrap();

        let mut app = KuraApp::new(lib.clone(), Mode::Browse, false).await.unwrap();
        assert_eq!(names(&app), ["plain", "moon baby", "office"]);
        assert_eq!(app.coverage.unread, 2);

        // A new query selects the best match.
        app.search_query = "boss".into();
        app.apply_search(true).await;
        assert_eq!(names(&app), ["office"]);
        assert_eq!(app.selected_hash(), Some(office.clone()));
        assert!(app.hits[0].snippet().is_some());

        // Clearing it keeps the selected image selected although its row moved.
        app.search_query.clear();
        app.apply_search(false).await;
        assert_eq!(app.selected_hash(), Some(office.clone()));
        assert_eq!(app.scroll_list.selected(), Some(2));

        // Another process captions an image: the next poll picks it up.
        app.search_query = "moon".into();
        app.apply_search(true).await;
        assert_eq!(app.selected_hash(), Some(moon.clone()));
        app.search_query = "walks".into();
        app.apply_search(true).await;
        assert_eq!(names(&app), ["office"]);
        let _ = lib.set_caption(&moon, caption("a boss walks by")).await.unwrap();
        app.refresh_if_changed().await;
        assert_eq!(names(&app), ["moon baby", "office"], "ties keep library order, newest first");
        assert_eq!(app.selected_hash(), Some(office), "a refresh keeps the image selected as rows move");
        assert_eq!(app.scroll_list.selected(), Some(1));
        assert_eq!(app.seq, lib.head().await.unwrap());
    }

    #[test]
    fn snippets_render_matches_between_cuts() {
        let snippet = Snippet {
            leading_cut: true,
            segments: vec![
                Segment { text: "the ".into(), matched: false },
                Segment { text: "moon".into(), matched: true },
            ],
            trailing_cut: true,
        };
        let spans = snippet_spans(&snippet);
        assert_eq!(spans.iter().map(|s| s.content.as_ref()).collect::<String>(), "…the moon…");
        assert_ne!(spans[1].style, spans[2].style, "matched words stand out");
    }
}
