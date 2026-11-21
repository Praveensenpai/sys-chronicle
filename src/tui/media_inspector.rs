use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
    Frame,
};
use regex::Regex;

use super::media_tab::MediaTabState;
use crate::mal::history::AnimeSyncRecord;
use crate::mal::SyncStatus;

fn format_time(secs: u64) -> String {
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, mins, s)
    } else {
        format!("{:02}:{:02}", mins, s)
    }
}

pub(crate) fn clean_display_title(s: &str) -> String {
    let mut text = s.to_string();
    for ext in [".mkv", ".mp4", ".avi", ".webm", ".mov", ".m4v"] {
        if text.to_ascii_lowercase().ends_with(ext) {
            text.truncate(text.len() - ext.len());
            break;
        }
    }
    if let Ok(re) = Regex::new(r"\[[^\]]*\]") {
        text = re.replace_all(&text, " ").to_string();
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn build_title_lines(r: &AnimeSyncRecord, max_w: usize) -> Vec<Line<'static>> {
    let raw = r.mal_title.as_deref().unwrap_or(&r.canonical_title);
    let title_clean = clean_display_title(raw);
    let title_w = max_w.saturating_sub(2).max(12);

    let mut lines = vec![Line::from(Span::styled(
        super::truncate_name(&title_clean, title_w),
        Style::default()
            .fg(Color::Rgb(255, 215, 0))
            .add_modifier(Modifier::BOLD),
    ))];

    if let Some(ref ja) = r.mal_title_ja {
        lines.push(Line::from(vec![
            Span::styled("🇯🇵 ", Style::default()),
            Span::styled(
                super::truncate_name(ja, title_w.saturating_sub(4).max(10)),
                Style::default().fg(Color::Rgb(180, 190, 220)),
            ),
        ]));
    } else if let Some(ref en) = r.mal_title_en {
        if en != &title_clean {
            lines.push(Line::from(vec![
                Span::styled("🇬🇧 ", Style::default()),
                Span::styled(
                    super::truncate_name(en, title_w.saturating_sub(4).max(10)),
                    Style::default().fg(Color::Rgb(150, 160, 180)),
                ),
            ]));
        }
    }
    lines
}

fn build_stats_lines(r: &AnimeSyncRecord, max_w: usize) -> Vec<Line<'static>> {
    let pos_str = format!(
        "{} / {}",
        format_time(r.position_secs.unwrap_or(0)),
        format_time(r.duration_secs)
    );
    let watch_str = format!(
        "{} / {} ({:.1}%)",
        format_time(r.watched_secs),
        format_time(r.duration_secs),
        r.watch_pct
    );
    let file_str = super::truncate_name(&r.raw_title, max_w.saturating_sub(10).max(12));
    let id_label = r
        .mal_anime_id
        .map(|id| format!("#{}", id))
        .unwrap_or_else(|| "Local".into());

    vec![
        Line::from(vec![
            Span::styled("Seek:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(pos_str, Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("Watched: ", Style::default().fg(Color::DarkGray)),
            Span::styled(watch_str, Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("File:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(file_str, Style::default().fg(Color::Gray)),
        ]),
        Line::from(vec![
            Span::styled("MAL ID:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(id_label, Style::default().fg(Color::Cyan)),
            Span::styled(format!("  {}", r.last_updated), Style::default().fg(Color::DarkGray)),
        ]),
    ]
}

fn build_media_details(r: &AnimeSyncRecord, max_w: usize) -> Vec<Line<'static>> {
    let mut lines = build_title_lines(r, max_w);
    let (badge_text, badge_color) = match &r.status {
        SyncStatus::AheadOnMal { mal_episode } => (format!("MAL: Ep {}", mal_episode), Color::Cyan),
        _ => (r.status.badge().0.to_string(), r.status.badge().1),
    };

    lines.push(Line::from(vec![
        Span::styled(
            format!(" Ep {} ", r.episode),
            Style::default()
                .bg(Color::Rgb(40, 45, 65))
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            format!(
                "(Total: {}) ",
                r.mal_total_episodes
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "?".into())
            ),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("[{}]", badge_text),
            Style::default().fg(badge_color).add_modifier(Modifier::BOLD),
        ),
    ]));

    lines.extend(build_stats_lines(r, max_w));
    lines
}

fn render_gauges(f: &mut Frame, r: &AnimeSyncRecord, pos_area: Rect, watched_area: Rect) {
    let pos_secs = r.position_secs.unwrap_or(0);
    let pos_pct = if r.duration_secs > 0 {
        (pos_secs as f32 / r.duration_secs as f32 * 100.0).min(100.0) as u16
    } else {
        0
    };
    let pos_label = format!(
        "{} / {} (Seek Head: {}%)",
        format_time(pos_secs),
        format_time(r.duration_secs),
        pos_pct
    );
    let pos_gauge = Gauge::default()
        .block(
            Block::default()
                .title(" ⏱️ Current Playback Position (Seek Head) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .gauge_style(
            Style::default()
                .fg(Color::Yellow)
                .bg(Color::Rgb(30, 30, 30)),
        )
        .label(pos_label)
        .percent(pos_pct);
    f.render_widget(pos_gauge, pos_area);

    let watched_pct = r.watch_pct.clamp(0.0, 100.0) as u16;
    let watched_label = format!(
        "{} / {} (Actual Watched: {:.1}%)",
        format_time(r.watched_secs),
        format_time(r.duration_secs),
        r.watch_pct
    );
    let watched_color = if r.watch_pct >= 80.0 {
        Color::Green
    } else {
        Color::Cyan
    };
    let watched_gauge = Gauge::default()
        .block(
            Block::default()
                .title(" 🛡️ Actual Watched Duration (Anti-Cheat: 80% needed) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(watched_color)),
        )
        .gauge_style(
            Style::default()
                .fg(watched_color)
                .bg(Color::Rgb(30, 30, 30)),
        )
        .label(watched_label)
        .percent(watched_pct);
    f.render_widget(watched_gauge, watched_area);
}

fn exp_lines(title: &'static str, color: Color, detail: String, note: &'static str) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            title,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(detail, Style::default().fg(Color::Gray))),
        Line::from(Span::styled(note, Style::default().fg(Color::DarkGray))),
    ]
}

fn build_sync_explanation(r: &AnimeSyncRecord) -> Vec<Line<'static>> {
    match &r.status {
        SyncStatus::AheadOnMal { mal_episode } => exp_lines(
            "⏩ MAL Progress is Ahead!",
            Color::Cyan,
            format!("• You watched Ep {}, but MAL has Ep {}.", r.episode, mal_episode),
            "• Protected from downgrade. Press 'f' to force overwrite.",
        ),
        SyncStatus::Synced => exp_lines(
            "✔ Synced with MyAnimeList",
            Color::Green,
            format!("Episode {} recorded on your MyAnimeList account.", r.episode),
            "• Your progress is fully synchronized.",
        ),
        SyncStatus::ThresholdMet => exp_lines(
            "⏳ 80% Threshold Reached",
            Color::Yellow,
            "Episode completed watch threshold.".into(),
            "• Press 's' or 'Enter' to sync to MyAnimeList.",
        ),
        SyncStatus::Watching => exp_lines(
            "▶ Watching in Progress",
            Color::Blue,
            format!("Currently at {:.1}% unique watch coverage.", r.watch_pct),
            "• Watch up to 80% to trigger automatic MAL sync.",
        ),
        SyncStatus::Failed { error } => vec![
            Line::from(Span::styled(
                "✖ Sync Failed",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(error.clone(), Style::default().fg(Color::Red))),
            Line::from(Span::styled("Press 's' to retry manual sync.", Style::default().fg(Color::Yellow))),
        ],
        SyncStatus::NonSyncable { reason } => exp_lines(
            "⭐ Non-Syncable Media (Not an Anime Episode)",
            Color::Magenta,
            format!("Category: {}. Bonus feature, special, or non-anime.", reason),
            "• SysChronicle tracks locally, but skips MAL sync.",
        ),
    }
}

fn render_inspector_top(
    f: &mut Frame,
    area: Rect,
    r: &AnimeSyncRecord,
    show_cover: bool,
    state: &mut MediaTabState,
) {
    if show_cover {
        let top_split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(26), Constraint::Min(20)])
            .split(area);

        let cover_block = Block::default()
            .title(" 🎨 Cover ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(100, 110, 140)));
        let cover_inner = cover_block.inner(top_split[0]);
        f.render_widget(cover_block, top_split[0]);
        state.cover_cache.render(f, cover_inner, r);

        let details = build_media_details(r, top_split[1].width as usize);
        f.render_widget(Paragraph::new(details), top_split[1]);
    } else {
        let details = build_media_details(r, area.width as usize);
        f.render_widget(Paragraph::new(details), area);
    }
}

fn render_status_footer(f: &mut Frame, area: Rect, msg: Option<&(String, std::time::Instant)>) {
    let status_text = if let Some((m, _)) = msg {
        Line::from(Span::styled(
            m,
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
    f.render_widget(Paragraph::new(status_text), area);
}

pub fn render_anime_inspector(f: &mut Frame, area: Rect, state: &mut MediaTabState) {
    let block = Block::default()
        .title(" 🔍 Anime & MAL Inspector ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));

    let Some(r) = state.selected_record().cloned() else {
        let empty = Paragraph::new("  Select an anime to inspect").block(block);
        f.render_widget(empty, area);
        return;
    };

    let show_cover = area.width >= 46 && area.height >= 22;
    let top_len = if show_cover {
        13.min(area.height.saturating_sub(12))
    } else {
        7
    };

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(top_len),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(2),
        ])
        .margin(1)
        .split(area);

    f.render_widget(block, area);

    render_inspector_top(f, inner[0], &r, show_cover, state);
    render_gauges(f, &r, inner[1], inner[2]);

    let exp_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    f.render_widget(
        Paragraph::new(build_sync_explanation(&r))
            .block(exp_block)
            .wrap(Wrap { trim: true }),
        inner[3],
    );

    render_status_footer(f, inner[4], state.status_message.as_ref());
}
