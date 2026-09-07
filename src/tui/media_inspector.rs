use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, Paragraph, Wrap},
    Frame,
};

use super::media_tab::MediaTabState;
use crate::mal::SyncStatus;

pub fn render_anime_inspector(f: &mut Frame, area: Rect, state: &MediaTabState) {
    let block = Block::default()
        .title(" 🔍 Anime & MAL Inspector ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));

    let Some(r) = state.selected_record() else {
        let empty = Paragraph::new("  Select an anime to inspect").block(block);
        f.render_widget(empty, area);
        return;
    };

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(3),
        ])
        .margin(1)
        .split(area);

    f.render_widget(block, area);

    let details = vec![
        Line::from(vec![
            Span::styled("Canonical: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                &r.canonical_title,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Episode:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("Episode {}", r.episode),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(
                format!(
                    " (Total: {})",
                    r.mal_total_episodes
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "?".into())
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled("Coverage:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!(
                    "{}s / {}s ({:.1}%)",
                    r.watched_secs, r.duration_secs, r.watch_pct
                ),
                Style::default().fg(Color::Green),
            ),
        ]),
        Line::from(vec![
            Span::styled("File:      ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                if r.raw_title.len() > 34 {
                    format!("{}...", &r.raw_title[..31])
                } else {
                    r.raw_title.clone()
                },
                Style::default().fg(Color::Gray),
            ),
        ]),
        Line::from(vec![
            Span::styled("Updated:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(&r.last_updated, Style::default().fg(Color::DarkGray)),
        ]),
    ];
    f.render_widget(Paragraph::new(details), inner[0]);

    let gauge = Gauge::default()
        .block(
            Block::default()
                .title(" Timeline Watch Progress (80% needed) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .gauge_style(
            Style::default()
                .fg(if r.watch_pct >= 80.0 {
                    Color::Green
                } else {
                    Color::Cyan
                })
                .bg(Color::Rgb(30, 30, 30)),
        )
        .percent(r.watch_pct.clamp(0.0, 100.0) as u16);
    f.render_widget(gauge, inner[1]);

    let sync_explanation = match &r.status {
        SyncStatus::AheadOnMal { mal_episode } => vec![
            Line::from(Span::styled(
                "⏩ MAL Progress is Ahead!",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!(
                    "• You watched Episode {}, but MAL already has Episode {} recorded.",
                    r.episode, mal_episode
                ),
                Style::default().fg(Color::Gray),
            )),
            Line::from(Span::styled(
                "• Automatic sync protected your MAL list from being downgraded.",
                Style::default().fg(Color::Green),
            )),
            Line::from(Span::styled(
                format!(
                    "• Press 'f' to force update MAL to Episode {} anyway.",
                    r.episode
                ),
                Style::default().fg(Color::Yellow),
            )),
        ],
        SyncStatus::Synced => vec![
            Line::from(Span::styled(
                "✔ Synced with MyAnimeList",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!(
                    "Episode {} is successfully recorded on your MyAnimeList account.",
                    r.episode
                ),
                Style::default().fg(Color::Gray),
            )),
        ],
        SyncStatus::ThresholdMet => vec![
            Line::from(Span::styled(
                "⏳ 80% Threshold Reached",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Ready to sync. Press 's' or 'Enter' to sync to MyAnimeList.",
                Style::default().fg(Color::White),
            )),
        ],
        SyncStatus::Watching => vec![
            Line::from(Span::styled(
                "▶ Watching in Progress",
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!(
                    "Currently at {:.1}% unique timeline coverage. Reach 80% to auto-sync.",
                    r.watch_pct
                ),
                Style::default().fg(Color::Gray),
            )),
        ],
        SyncStatus::Failed { error } => vec![
            Line::from(Span::styled(
                "✖ Sync Failed",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(error, Style::default().fg(Color::Red))),
            Line::from(Span::styled(
                "Press 's' to retry manual sync.",
                Style::default().fg(Color::Yellow),
            )),
        ],
    };

    let exp_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    f.render_widget(
        Paragraph::new(sync_explanation)
            .block(exp_block)
            .wrap(Wrap { trim: true }),
        inner[2],
    );

    let status_text = if let Some((msg, _)) = &state.status_message {
        Line::from(Span::styled(
            msg,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ))
    } else {
        Line::from(Span::styled(
            "[s/Enter] Sync   [f] Force Sync   [r] Refresh",
            Style::default().fg(Color::DarkGray),
        ))
    };
    f.render_widget(Paragraph::new(status_text), inner[3]);
}

pub fn render_force_sync_modal(f: &mut Frame, area: Rect, state: &MediaTabState) {
    let block = Block::default()
        .title(" ⚠️ Confirm Force Overwrite ")
        .borders(Borders::ALL)
        .border_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    let popup_area = centered_rect(50, 30, area);
    f.render_widget(Clear, popup_area);

    let ep = state.selected_record().map(|r| r.episode).unwrap_or(0);
    let mal_ep = state
        .selected_record()
        .and_then(|r| match r.status {
            SyncStatus::AheadOnMal { mal_episode } => Some(mal_episode),
            _ => None,
        })
        .unwrap_or(0);

    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("MyAnimeList is currently at Episode {}.", mal_ep),
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            format!(
                "Force syncing will downgrade/set your MAL progress to Episode {}.",
                ep
            ),
            Style::default().fg(Color::LightRed),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Are you sure you want to force overwrite?",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(" [f] ", Style::default().bg(Color::Red).fg(Color::White)),
            Span::raw(" Confirm Force Sync    "),
            Span::styled(
                " [Esc] ",
                Style::default().bg(Color::DarkGray).fg(Color::White),
            ),
            Span::raw(" Cancel"),
        ]),
    ];

    let p = Paragraph::new(text)
        .block(block)
        .alignment(Alignment::Center);
    f.render_widget(p, popup_area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
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
        .split(popup_layout[1])[1]
}
