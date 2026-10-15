use chrono::{Datelike, Local, NaiveDate, NaiveDateTime};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};
use std::collections::HashMap;
use std::time::Instant;

use super::media_inspector::{render_anime_inspector, render_force_sync_modal};
use crate::mal::{AnimeSyncHistory, AnimeSyncRecord, MalHandler, SyncStatus};

const TIMESTAMP_FMT: &str = "%Y-%m-%d %H:%M:%S";
const TITLE_COL_WIDTH: usize = 22;

/// Temporal bucket used to group watching sessions in the anime list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupMode {
    Day,
    Week,
    Month,
    Flat,
}

impl GroupMode {
    pub fn next(self) -> Self {
        match self {
            GroupMode::Day => GroupMode::Week,
            GroupMode::Week => GroupMode::Month,
            GroupMode::Month => GroupMode::Flat,
            GroupMode::Flat => GroupMode::Day,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            GroupMode::Day => "Day",
            GroupMode::Week => "Week",
            GroupMode::Month => "Month",
            GroupMode::Flat => "Flat",
        }
    }
}

/// A rendered list entry: either a non-selectable group header or a record index.
enum MediaRow {
    Header(String),
    Record(usize),
}

pub struct MediaTabState {
    pub records: Vec<AnimeSyncRecord>,
    pub selected_index: usize,
    pub status_message: Option<(String, Instant)>,
    pub confirming_force_sync: bool,
    pub list_state: ListState,
    pub group_mode: GroupMode,
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
            group_mode: GroupMode::Day,
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

    pub fn cycle_group_mode(&mut self) {
        self.group_mode = self.group_mode.next();
        self.set_status(format!(
            "Grouped watching sessions by {}",
            self.group_mode.label()
        ));
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

fn first_of_week(date: NaiveDate) -> NaiveDate {
    date - chrono::Duration::days(date.weekday().num_days_from_monday() as i64)
}

fn first_of_month(date: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), date.month(), 1).unwrap_or(date)
}

fn weeks_between(today: NaiveDate, date: NaiveDate) -> i64 {
    (first_of_week(today) - first_of_week(date)).num_days() / 7
}

fn months_between(today: NaiveDate, date: NaiveDate) -> i32 {
    let a = first_of_month(today);
    let b = first_of_month(date);
    (a.year() - b.year()) * 12 + a.month() as i32 - b.month() as i32
}

fn group_label(mode: GroupMode, ts: NaiveDateTime, today: NaiveDate) -> String {
    let date = ts.date();
    match mode {
        GroupMode::Flat => "All Sessions".to_string(),
        GroupMode::Day => match (today - date).num_days() {
            0 => "Today".to_string(),
            1 => "Yesterday".to_string(),
            _ => date.format("%A, %d %b %Y").to_string(),
        },
        GroupMode::Week => match weeks_between(today, date) {
            0 => format!("This Week (from {})", first_of_week(today).format("%d %b")),
            1 => format!("Last Week (from {})", first_of_week(date).format("%d %b")),
            _ => format!("Week of {}", first_of_week(date).format("%d %b %Y")),
        },
        GroupMode::Month => match months_between(today, date) {
            0 => "This Month".to_string(),
            1 => "Last Month".to_string(),
            _ => date.format("%B %Y").to_string(),
        },
    }
}

fn parse_timestamp(ts: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(ts, TIMESTAMP_FMT).ok()
}

/// Latest activity timestamp per anime so all episodes of one show stay in a single bucket.
fn anime_latest_activity(records: &[AnimeSyncRecord]) -> HashMap<String, NaiveDateTime> {
    let mut latest: HashMap<String, NaiveDateTime> = HashMap::new();
    for record in records {
        let Some(ts) = parse_timestamp(&record.last_updated) else {
            continue;
        };
        let key = record.canonical_title.trim().to_lowercase();
        latest
            .entry(key)
            .and_modify(|existing| {
                if ts > *existing {
                    *existing = ts;
                }
            })
            .or_insert(ts);
    }
    latest
}

fn build_rows(records: &[AnimeSyncRecord], mode: GroupMode, today: NaiveDate) -> Vec<MediaRow> {
    if mode == GroupMode::Flat {
        return (0..records.len()).map(MediaRow::Record).collect();
    }

    let latest = anime_latest_activity(records);
    let mut rows = Vec::with_capacity(records.len() + 8);
    let mut current: Option<String> = None;

    for (index, record) in records.iter().enumerate() {
        let key = record.canonical_title.trim().to_lowercase();
        let label = latest
            .get(&key)
            .copied()
            .or_else(|| parse_timestamp(&record.last_updated))
            .map(|ts| group_label(mode, ts, today))
            .unwrap_or_else(|| "Unknown Date".to_string());

        if current.as_deref() != Some(label.as_str()) {
            rows.push(MediaRow::Header(label.clone()));
            current = Some(label);
        }
        rows.push(MediaRow::Record(index));
    }

    rows
}

fn header_item(label: &str) -> ListItem<'static> {
    ListItem::new(Line::from(Span::styled(
        format!("  {}  ", label),
        Style::default()
            .fg(Color::Black)
            .bg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
    )))
}

fn record_item(record: &AnimeSyncRecord, index: usize, is_sel: bool) -> ListItem<'static> {
    let cursor = if is_sel { "► " } else { "  " };

    let (badge_text, badge_color) = match &record.status {
        SyncStatus::AheadOnMal { mal_episode } => (format!("MAL: Ep {}", mal_episode), Color::Cyan),
        _ => (record.status.badge().0.to_string(), record.status.badge().1),
    };

    let title_display = super::truncate_name(&record.canonical_title, TITLE_COL_WIDTH);

    ListItem::new(Line::from(vec![
        Span::styled(
            cursor,
            Style::default().fg(if is_sel {
                Color::Yellow
            } else {
                Color::DarkGray
            }),
        ),
        Span::styled(
            format!("#{:02} ", index + 1),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("{:<width$} ", title_display, width = TITLE_COL_WIDTH),
            Style::default()
                .fg(if is_sel { Color::White } else { Color::Gray })
                .add_modifier(if is_sel {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ),
        Span::styled(
            format!("Ep {:<2} ", record.episode),
            Style::default().fg(Color::Yellow),
        ),
        Span::styled(
            format!("{:>5.1}% ", record.watch_pct),
            Style::default().fg(if record.watch_pct >= 80.0 {
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
    ]))
}

fn render_empty_media(f: &mut Frame, area: Rect, block: Block<'_>) {
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
        .title(format!(
            " 🎬 Anime & Media Chronicle [Group: {} (g)] ",
            state.group_mode.label()
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    if state.records.is_empty() {
        render_empty_media(f, area, block);
        return;
    }

    let rows = build_rows(&state.records, state.group_mode, Local::now().date_naive());
    let mut items: Vec<ListItem> = Vec::with_capacity(rows.len());
    let mut selected_row = None;

    for (row_index, row) in rows.iter().enumerate() {
        match row {
            MediaRow::Header(label) => items.push(header_item(label)),
            MediaRow::Record(record_index) => {
                let is_sel = *record_index == state.selected_index;
                if is_sel {
                    selected_row = Some(row_index);
                }
                items.push(record_item(
                    &state.records[*record_index],
                    *record_index,
                    is_sel,
                ));
            }
        }
    }

    state.list_state.select(selected_row);

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(Color::Rgb(30, 35, 50)));

    f.render_stateful_widget(list, area, &mut state.list_state);
}

#[cfg(test)]
mod tests;
