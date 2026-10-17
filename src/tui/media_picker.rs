use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use crate::mal::client::MalAnimeNode;
use crate::mal::overrides::reassign_history;
use crate::mal::{AnimeOverrides, MalAuth, MalClient};

/// Delay applied after the last keystroke before hitting the MAL API.
/// Prevents one network request per typed character.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(300);

struct SearchRequest {
    generation: u64,
    query: String,
}

/// Result handed back from the background search thread.
struct SearchOutcome {
    generation: u64,
    results: Vec<MalAnimeNode>,
    error: Option<String>,
}

/// Modal that lets the user pick the MAL entry a folder should map to.
pub struct MediaPickerState {
    pub visible: bool,
    pub query: String,
    pub results: Vec<MalAnimeNode>,
    pub selected: usize,
    pub searching: bool,
    pub status: Option<String>,
    folder: Option<PathBuf>,
    generation: u64,
    pending_since: Option<Instant>,
    request_tx: Sender<SearchRequest>,
    outcome_rx: Receiver<SearchOutcome>,
}

impl MediaPickerState {
    pub fn new() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<SearchRequest>();
        let (outcome_tx, outcome_rx) = mpsc::channel::<SearchOutcome>();
        std::thread::spawn(move || search_worker(request_rx, outcome_tx));

        Self {
            visible: false,
            query: String::new(),
            results: Vec::new(),
            selected: 0,
            searching: false,
            status: None,
            folder: None,
            generation: 0,
            pending_since: None,
            request_tx,
            outcome_rx,
        }
    }

    /// Open the modal for a folder, seeded with a starting search query.
    pub fn open(&mut self, folder: Option<PathBuf>, seed_query: &str) {
        self.visible = true;
        self.folder = folder;
        self.query = seed_query.to_string();
        self.results.clear();
        self.selected = 0;
        self.status = None;
        self.start_search();
    }

    pub fn close(&mut self) {
        self.visible = false;
        self.searching = false;
        self.pending_since = None;
    }

    pub fn push_char(&mut self, c: char) {
        self.query.push(c);
        self.start_search();
    }

    pub fn pop_char(&mut self) {
        self.query.pop();
        self.start_search();
    }

    /// Re-run the current query, bypassing the debounce. Bound to a dedicated
    /// key so the user can retry without editing the text.
    pub fn retry(&mut self) {
        self.start_search();
        self.pending_since = Some(Instant::now() - SEARCH_DEBOUNCE);
    }

    /// True when there is nothing to bind, so `Enter` should re-query instead.
    pub fn needs_query(&self) -> bool {
        self.results.is_empty()
    }

    pub fn select_next(&mut self) {
        if !self.results.is_empty() {
            self.selected = (self.selected + 1) % self.results.len();
        }
    }

    pub fn select_prev(&mut self) {
        if !self.results.is_empty() {
            self.selected = if self.selected == 0 {
                self.results.len() - 1
            } else {
                self.selected - 1
            };
        }
    }

    /// Mark the current query dirty and arm the debounce timer.
    fn start_search(&mut self) {
        let query = self.query.trim().to_string();
        self.results.clear();
        self.selected = 0;
        self.status = None;
        self.generation += 1;
        self.pending_since = None;
        if query.is_empty() {
            self.searching = false;
            return;
        }

        self.searching = true;
        self.pending_since = Some(Instant::now());
    }

    /// Dispatch the pending request once the quiet period has elapsed.
    pub fn tick(&mut self) {
        let Some(since) = self.pending_since else {
            return;
        };
        if since.elapsed() < SEARCH_DEBOUNCE {
            return;
        }
        self.pending_since = None;

        let query = self.query.trim().to_string();
        if query.is_empty() {
            self.searching = false;
            return;
        }

        let request = SearchRequest {
            generation: self.generation,
            query,
        };
        // The worker thread lives for the process lifetime, so a failed send
        // only happens during shutdown. Surface it instead of hanging.
        if self.request_tx.send(request).is_err() {
            self.searching = false;
            self.status = Some("Search worker stopped unexpectedly.".to_string());
        }
    }

    /// Collect a finished background search, if any. Call once per UI loop.
    pub fn poll(&mut self) {
        loop {
            match self.outcome_rx.try_recv() {
                Ok(outcome) => {
                    if outcome.generation != self.generation {
                        continue;
                    }
                    self.searching = false;
                    self.results = outcome.results;
                    self.selected = 0;
                    if let Some(err) = outcome.error {
                        self.status = Some(err);
                    }
                    return;
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    if self.searching {
                        self.searching = false;
                        self.status = Some("Search worker stopped unexpectedly.".to_string());
                    }
                    return;
                }
            }
        }
    }

    /// Persist the selected entry as the folder override.
    /// Returns a status message describing what happened.
    pub fn apply(&mut self) -> Option<String> {
        let Some(folder) = self.folder.clone() else {
            return Some("This record has no file path to bind.".to_string());
        };
        let Some(anime) = self.results.get(self.selected).cloned() else {
            return Some("Select a MyAnimeList entry first.".to_string());
        };

        let total = if anime.num_episodes > 0 {
            Some(anime.num_episodes)
        } else {
            None
        };

        let mut overrides = match AnimeOverrides::load() {
            Ok(o) => o,
            Err(e) => return Some(format!("Could not load overrides: {e}")),
        };
        overrides.set(&folder, anime.id, &anime.title, total);
        if let Err(e) = overrides.save() {
            return Some(format!("Could not save override: {e}"));
        }

        let touched = reassign_history(&folder, anime.id, &anime.title, total).unwrap_or(0);
        self.close();
        Some(format!(
            "Bound {} -> {} ({}); updated {} record(s)",
            folder.display(),
            anime.id,
            anime.title,
            touched
        ))
    }
}

/// Single long-lived worker. Consumes requests in order and always replies
/// with the newest query it was handed, so no request is ever lost to a race.
fn search_worker(request_rx: Receiver<SearchRequest>, outcome_tx: Sender<SearchOutcome>) {
    let mut latest: Option<SearchRequest> = None;
    loop {
        if latest.is_none() {
            match request_rx.recv() {
                Ok(req) => latest = Some(req),
                Err(_) => return,
            }
        }

        // Drain anything queued while we were busy: only the newest matters.
        while let Ok(next) = request_rx.try_recv() {
            latest = Some(next);
        }

        let Some(req) = latest.take() else { continue };
        let (results, error) = search_blocking(&req.query);
        let outcome = SearchOutcome {
            generation: req.generation,
            results,
            error,
        };
        if outcome_tx.send(outcome).is_err() {
            return;
        }
    }
}

fn search_blocking(query: &str) -> (Vec<MalAnimeNode>, Option<String>) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => return (Vec::new(), Some(format!("Runtime error: {e}"))),
    };

    runtime.block_on(async {
        let mut config = match MalAuth::load_config() {
            Ok(c) => c,
            Err(e) => return (Vec::new(), Some(format!("MAL config error: {e}"))),
        };
        let token = match MalAuth::get_valid_token(&mut config).await {
            Ok(t) => t,
            Err(e) => return (Vec::new(), Some(format!("MAL auth error: {e}"))),
        };
        let client = match MalClient::new(token) {
            Ok(c) => c,
            Err(e) => return (Vec::new(), Some(format!("MAL client error: {e}"))),
        };
        match client.search_anime(query).await {
            Ok(results) => (results, None),
            Err(e) => (Vec::new(), Some(format!("Search failed: {e}"))),
        }
    })
}

pub fn render_media_picker(f: &mut Frame, area: Rect, state: &MediaPickerState) {
    if !state.visible {
        return;
    }

    let popup = centered_rect(76, 68, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" 🔗 Map Folder to MyAnimeList ")
        .borders(Borders::ALL)
        .border_style(
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        );

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(2),
        ])
        .margin(1)
        .split(popup);

    f.render_widget(block, popup);

    let folder_label = state
        .folder
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(no folder)".to_string());
    let hint = if state.searching {
        "searching…".to_string()
    } else if let Some(ref msg) = state.status {
        msg.clone()
    } else if state.results.is_empty() {
        "0 result(s) — press Ctrl+R to retry".to_string()
    } else {
        format!("{} result(s)", state.results.len())
    };

    let search_box = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Folder: ", Style::default().fg(Color::DarkGray)),
            Span::styled(folder_label, Style::default().fg(Color::Gray)),
        ]),
        Line::from(vec![
            Span::styled("Search: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}_", state.query),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("   {}", hint), Style::default().fg(Color::DarkGray)),
        ]),
    ])
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(search_box, inner[0]);

    let items: Vec<ListItem> = state
        .results
        .iter()
        .enumerate()
        .map(|(idx, node)| {
            let selected = idx == state.selected;
            let style = if selected {
                Style::default()
                    .bg(Color::Rgb(35, 30, 50))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:>8}  ", node.id),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("[{:<7}] ", node.media_type.as_deref().unwrap_or("?")),
                    style,
                ),
                Span::styled(node.title.clone(), style),
                Span::styled(
                    if node.num_episodes > 0 {
                        format!("  ({} ep)", node.num_episodes)
                    } else {
                        String::new()
                    },
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" Search Results ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(list, inner[1]);

    let enter_label = if state.needs_query() {
        " search "
    } else {
        " bind folder "
    };
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(
            " ↑/↓ ",
            Style::default().bg(Color::DarkGray).fg(Color::White),
        ),
        Span::raw(" select   "),
        Span::styled(
            " Enter ",
            Style::default().bg(Color::Green).fg(Color::Black),
        ),
        Span::raw(enter_label),
        Span::styled(
            " Ctrl+R ",
            Style::default().bg(Color::Cyan).fg(Color::Black),
        ),
        Span::raw(" re-query   "),
        Span::styled(
            " Esc ",
            Style::default().bg(Color::DarkGray).fg(Color::White),
        ),
        Span::raw(" cancel "),
    ]))
    .alignment(Alignment::Center);
    f.render_widget(footer, inner[2]);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
