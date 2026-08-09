use anyhow::Result;
use chrono::{DateTime, Local};
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Wrap},
    Terminal,
};
use std::io::stdout;
use std::time::Duration;
use sysinfo::System;

use crate::monitor::{MetricsMonitor, PowerMonitor, WindowMonitor};

pub fn run_status_tui() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut sys = System::new_all();

    let mut is_paused = false;
    let mut paused_at: Option<DateTime<Local>> = None;

    // Cached frozen snapshot for paused state
    let mut cached_window = WindowMonitor::get_current_window();
    let mut cached_power = PowerMonitor::read_current_state();
    let mut cached_metrics = MetricsMonitor::sample_metrics(&mut sys);

    loop {
        let now = Local::now();

        // Sample current data if not paused
        if !is_paused {
            cached_window = WindowMonitor::get_current_window();
            cached_power = PowerMonitor::read_current_state();
            cached_metrics = MetricsMonitor::sample_metrics(&mut sys);
        }

        let window_info = &cached_window;
        let power_info = &cached_power;
        let metrics = &cached_metrics;

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints(
                    [
                        Constraint::Length(3), // Header
                        Constraint::Length(5), // Active Window & Battery
                        Constraint::Length(6), // CPU & RAM Gauges
                        Constraint::Min(6),    // Top Applications & Info
                        Constraint::Length(1), // Footer controls
                    ]
                    .as_ref(),
                )
                .split(f.size());

            // Header Status Badge
            let status_span = if is_paused {
                let elapsed_secs = match paused_at {
                    Some(t) => (now - t).num_seconds().max(0),
                    None => 0,
                };
                let relative_str = format_relative_time(elapsed_secs);
                Span::styled(
                    format!(" ⏸️ PAUSED ({}) ", relative_str),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(
                    " 🟢 LIVE ",
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                )
            };

            let header = Paragraph::new(Line::from(vec![
                Span::styled(" ⏱️  SysChronicle ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled("v0.1.5", Style::default().fg(Color::DarkGray)),
                Span::raw(" | "),
                status_span,
                Span::raw(" | "),
                Span::styled("System Activity Dashboard", Style::default().fg(Color::Gray)),
            ]))
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)));
            f.render_widget(header, chunks[0]);

            // Top Row: Active Window & Power
            let top_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
                .split(chunks[1]);

            // Active Window Widget
            let (app_cls, app_title) = match window_info {
                Some((cls, title)) => (cls.as_str(), title.as_str()),
                None => ("None", "Idle / Unknown"),
            };

            let win_text = vec![
                Line::from(vec![
                    Span::styled("Application: ", Style::default().fg(Color::Gray)),
                    Span::styled(app_cls, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("Window Title: ", Style::default().fg(Color::Gray)),
                    Span::styled(app_title, Style::default().fg(Color::White)),
                ]),
            ];
            let win_border_color = if is_paused { Color::Yellow } else { Color::Blue };
            let win_widget = Paragraph::new(win_text)
                .block(Block::default().title(" 🪟 Active Window ").borders(Borders::ALL).border_style(Style::default().fg(win_border_color)))
                .wrap(Wrap { trim: true });
            f.render_widget(win_widget, top_chunks[0]);

            // Battery Widget
            let battery_lines = if let Some(pow) = power_info {
                let ac_str = if pow.ac_online { "Plugged in (AC)" } else { "Battery Power" };
                let color = if pow.capacity <= 20 {
                    Color::Red
                } else if pow.capacity <= 40 {
                    Color::Yellow
                } else {
                    Color::Green
                };
                vec![
                    Line::from(vec![
                        Span::styled("Capacity: ", Style::default().fg(Color::Gray)),
                        Span::styled(format!("{}%", pow.capacity), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                        Span::raw(" ("),
                        Span::styled(&pow.status, Style::default().fg(Color::Cyan)),
                        Span::raw(")"),
                    ]),
                    Line::from(vec![
                        Span::styled("Power Source: ", Style::default().fg(Color::Gray)),
                        Span::styled(ac_str, Style::default().fg(Color::Magenta)),
                    ]),
                ]
            } else {
                vec![Line::from(Span::raw("No battery detected"))]
            };
            let bat_widget = Paragraph::new(battery_lines)
                .block(Block::default().title(" 🔋 Power State ").borders(Borders::ALL).border_style(Style::default().fg(Color::Magenta)));
            f.render_widget(bat_widget, top_chunks[1]);

            // Middle Row: Gauges for CPU and RAM
            let gauge_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Length(3)].as_ref())
                .split(chunks[2]);

            // CPU Gauge
            let cpu_ratio = (metrics.cpu_pct / 100.0).clamp(0.0, 1.0);
            let cpu_gauge = Gauge::default()
                .block(Block::default().title(format!(" ⚡ CPU Utilization: {:.1}% ", metrics.cpu_pct)).borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)))
                .gauge_style(Style::default().fg(if metrics.cpu_pct > 75.0 { Color::Red } else { Color::Yellow }))
                .ratio(cpu_ratio as f64);
            f.render_widget(cpu_gauge, gauge_chunks[0]);

            // RAM Gauge
            let ram_ratio = (metrics.ram_pct / 100.0).clamp(0.0, 1.0);
            let ram_label = format!("{:.1}% ({} / {} MB)", metrics.ram_pct, metrics.ram_used_mb, metrics.ram_total_mb);
            let ram_gauge = Gauge::default()
                .block(Block::default().title(format!(" 🧠 RAM Utilization: {} ", ram_label)).borders(Borders::ALL).border_style(Style::default().fg(Color::Green)))
                .gauge_style(Style::default().fg(if metrics.ram_pct > 85.0 { Color::Red } else { Color::Green }))
                .ratio(ram_ratio as f64);
            f.render_widget(ram_gauge, gauge_chunks[1]);

            // Bottom Row: Top Applications List
            let app_items: Vec<ListItem> = metrics
                .top_apps
                .iter()
                .enumerate()
                .map(|(idx, app)| {
                    let style = match idx {
                        0 => Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
                        1 => Style::default().fg(Color::LightYellow),
                        2 => Style::default().fg(Color::LightCyan),
                        _ => Style::default().fg(Color::White),
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(format!(" #{} ", idx + 1), Style::default().fg(Color::DarkGray)),
                        Span::styled(app, style),
                    ]))
                })
                .collect();

            let apps_list = List::new(app_items)
                .block(Block::default().title(" 📊 Top Memory Consuming Applications ").borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)));
            f.render_widget(apps_list, chunks[3]);

            // Footer
            let pause_action_str = if is_paused { "resume" } else { "pause" };
            let footer = Paragraph::new(Line::from(vec![
                Span::styled(" Press ", Style::default().fg(Color::DarkGray)),
                Span::styled("p", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" to {} dashboard | ", pause_action_str), Style::default().fg(Color::DarkGray)),
                Span::styled("q", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(" or ", Style::default().fg(Color::DarkGray)),
                Span::styled("Esc", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(" to exit", Style::default().fg(Color::DarkGray)),
            ]));
            f.render_widget(footer, chunks[4]);
        })?;

        // Poll for keypress with 1s timeout
        if event::poll(Duration::from_secs(1))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('p') | KeyCode::Char('P') => {
                        if is_paused {
                            is_paused = false;
                            paused_at = None;
                        } else {
                            is_paused = true;
                            paused_at = Some(Local::now());
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

fn format_relative_time(secs: i64) -> String {
    if secs < 60 {
        format!("{}s ago", secs)
    } else {
        let mins = secs / 60;
        let s = secs % 60;
        format!("{}m {}s ago", mins, s)
    }
}
