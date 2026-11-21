use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use super::media_tab::MediaTabState;
use crate::mal::SyncStatus;

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
