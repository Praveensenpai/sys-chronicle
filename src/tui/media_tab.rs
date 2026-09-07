use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};
use std::time::Instant;

use super::media_inspector::{render_anime_inspector, render_force_sync_modal};
use crate::mal::{AnimeSyncHistory, AnimeSyncRecord, MalHandler, SyncStatus};

pub struct MediaTabState {
    pub records: Vec<AnimeSyncRecord>,
    pub selected_index: usize,
    pub status_message: Option<(String, Instant)>,
    pub confirming_force_sync: bool,
    pub list_state: ListState,
}

impl MediaTabState {
    pub fn new() -> Self {
        let records = AnimeSyncHistory::load_all().unwrap_or_default();
        let mut list_state = ListState::default();
        if !records.is_empty() {
            list_state.select(Some(0));
        }
        Self {
            records,
            selected_index: 0,
            status_message: None,
            confirming_force_sync: false,
            list_state,
        }
    }

    pub fn refresh(&mut self) {
        self.records = AnimeSyncHistory::load_all().unwrap_or_default();
        if self.records.is_empty() {
            self.selected_index = 0;
            self.list_state.select(None);
        } else {
            self.selected_index = self.selected_index.min(self.records.len() - 1);
            self.list_state.select(Some(self.selected_index));
        }
    }

    pub fn select_next(&mut self) {
        if self.records.is_empty() {
            return;
        }
        if self.selected_index + 1 < self.records.len() {
            self.selected_index += 1;
        } else {
            self.selected_index = 0;
        }
        self.list_state.select(Some(self.selected_index));
    }

    pub fn select_prev(&mut self) {
        if self.records.is_empty() {
            return;
        }
        if self.selected_index > 0 {
            self.selected_index -= 1;
        } else {
            self.selected_index = self.records.len() - 1;
        }
        self.list_state.select(Some(self.selected_index));
    }

    pub fn selected_record(&self) -> Option<&AnimeSyncRecord> {
        self.records.get(self.selected_index)
    }

    pub fn set_status(&mut self, msg: String) {
        self.status_message = Some((msg, Instant::now()));
    }

    pub fn trigger_sync(&mut self, force: bool) {
        let Some(record) = self.selected_record().cloned() else {
            self.set_status("No anime selected to sync.".into());
            return;
        };

        if matches!(record.status, SyncStatus::AheadOnMal { .. }) && !force {
            self.confirming_force_sync = true;
            return;
        }

        self.confirming_force_sync = false;
        let title = record.canonical_title.clone();
        self.set_status(format!("Syncing \"{}\" to MAL in background...", title));

        let raw = record.path.unwrap_or(record.raw_title);
        let duration = record.duration_secs;
        let watched = record.watched_secs;

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = MalHandler::sync_media_with_options(&raw, duration, watched, force).await;
            });
        } else {
            std::thread::spawn(move || {
                let mut cmd = std::process::Command::new("sys-chronicle-mal");
                cmd.arg("sync").arg(&raw);
                if force {
                    cmd.arg("--force");
                }
                let _ = cmd.status();
            });
        }
    }
}

pub fn render_media_tab(f: &mut Frame, area: Rect, state: &mut MediaTabState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(area);

    render_anime_list(f, chunks[0], state);
    render_anime_inspector(f, chunks[1], state);

    if state.confirming_force_sync {
        render_force_sync_modal(f, area, state);
    }
}

fn render_anime_list(f: &mut Frame, area: Rect, state: &mut MediaTabState) {
    let block = Block::default()
        .title(" 🎬 Anime & Media Chronicle ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    if state.records.is_empty() {
        let empty_msg = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No media playback history found.",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  ▶ Play an anime episode in MPV to track unique watch coverage.",
                Style::default().fg(Color::Gray),
            )),
            Line::from(Span::styled(
                "  ⚡ Once you reach 80% coverage, it automatically logs to MyAnimeList.",
                Style::default().fg(Color::Yellow),
            )),
        ])
        .block(block);
        f.render_widget(empty_msg, area);
        return;
    }

    let items: Vec<ListItem> = state
        .records
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let is_sel = i == state.selected_index;
            let cursor = if is_sel { "► " } else { "  " };

            let (badge_text, badge_color) = match &r.status {
                SyncStatus::AheadOnMal { mal_episode } => {
                    (format!("MAL: Ep {}", mal_episode), Color::Cyan)
                }
                _ => (r.status.badge().0.to_string(), r.status.badge().1),
            };

            let title_display = if r.canonical_title.len() > 22 {
                format!("{}...", &r.canonical_title[..19])
            } else {
                r.canonical_title.clone()
            };

            let line = Line::from(vec![
                Span::styled(
                    cursor,
                    Style::default().fg(if is_sel {
                        Color::Yellow
                    } else {
                        Color::DarkGray
                    }),
                ),
                Span::styled(
                    format!("#{:02} ", i + 1),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{:<22} ", title_display),
                    Style::default()
                        .fg(if is_sel { Color::White } else { Color::Gray })
                        .add_modifier(if is_sel {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
                Span::styled(
                    format!("Ep {:<2} ", r.episode),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    format!("{:>5.1}% ", r.watch_pct),
                    Style::default().fg(if r.watch_pct >= 80.0 {
                        Color::Green
                    } else {
                        Color::Blue
                    }),
                ),
                Span::styled(
                    format!("[{}]", badge_text),
                    Style::default()
                        .fg(badge_color)
                        .add_modifier(Modifier::BOLD),
                ),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(Color::Rgb(30, 35, 50)));

    f.render_stateful_widget(list, area, &mut state.list_state);
}
