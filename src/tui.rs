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
    widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};
use std::io::stdout;
use std::time::Duration;
use sysinfo::System;

use crate::monitor::metrics::AppDetail;
use crate::monitor::{MetricsMonitor, PowerMonitor, WindowMonitor};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LimitMode {
    Top10,
    Top25,
    All,
}

impl LimitMode {
    fn next(self) -> Self {
        match self {
            LimitMode::Top10 => LimitMode::Top25,
            LimitMode::Top25 => LimitMode::All,
            LimitMode::All => LimitMode::Top10,
        }
    }

    fn label(self) -> &'static str {
        match self {
            LimitMode::Top10 => "10",
            LimitMode::Top25 => "25",
            LimitMode::All => "All",
        }
    }

    fn limit(self) -> Option<usize> {
        match self {
            LimitMode::Top10 => Some(10),
            LimitMode::Top25 => Some(25),
            LimitMode::All => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortMetric {
    Ram,
    Cpu,
}

impl SortMetric {
    fn next(self) -> Self {
        match self {
            SortMetric::Ram => SortMetric::Cpu,
            SortMetric::Cpu => SortMetric::Ram,
        }
    }

    fn label(self) -> &'static str {
        match self {
            SortMetric::Ram => "RAM",
            SortMetric::Cpu => "CPU",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortOrder {
    Descending,
    Ascending,
}

impl SortOrder {
    fn toggle(self) -> Self {
        match self {
            SortOrder::Descending => SortOrder::Ascending,
            SortOrder::Ascending => SortOrder::Descending,
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            SortOrder::Descending => "↓",
            SortOrder::Ascending => "↑",
        }
    }
}

fn truncate_name(name: &str, max_len: usize) -> String {
    if name.chars().count() > max_len {
        let truncated: String = name.chars().take(max_len.saturating_sub(2)).collect();
        format!("{}..", truncated)
    } else {
        name.to_string()
    }
}

pub fn run_status_tui() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut sys = System::new_all();

    let mut is_paused = false;
    let mut paused_at: Option<DateTime<Local>> = None;
    let mut limit_mode = LimitMode::Top10;
    let mut sort_metric = SortMetric::Ram;
    let mut sort_order = SortOrder::Descending;

    let mut list_state = ListState::default();
    list_state.select(Some(0));

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

        // Apply sorting to app details
        let mut sorted_apps: Vec<AppDetail> = metrics.app_details.clone();
        match (sort_metric, sort_order) {
            (SortMetric::Ram, SortOrder::Descending) => {
                sorted_apps.sort_by_key(|a| std::cmp::Reverse(a.ram_mb));
            }
            (SortMetric::Ram, SortOrder::Ascending) => {
                sorted_apps.sort_by_key(|a| a.ram_mb);
            }
            (SortMetric::Cpu, SortOrder::Descending) => {
                sorted_apps.sort_by(|a, b| b.cpu_pct.partial_cmp(&a.cpu_pct).unwrap_or(std::cmp::Ordering::Equal));
            }
            (SortMetric::Cpu, SortOrder::Ascending) => {
                sorted_apps.sort_by(|a, b| a.cpu_pct.partial_cmp(&b.cpu_pct).unwrap_or(std::cmp::Ordering::Equal));
            }
        }

        // Slice app details based on active LimitMode
        let visible_apps = match limit_mode.limit() {
            Some(lim) => &sorted_apps[..sorted_apps.len().min(lim)],
            None => &sorted_apps,
        };

        // Ensure selection stays within valid bounds
        let visible_count = visible_apps.len();
        if visible_count > 0 {
            if let Some(selected) = list_state.selected() {
                if selected >= visible_count {
                    list_state.select(Some(visible_count - 1));
                }
            } else {
                list_state.select(Some(0));
            }
        }

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints(
                    [
                        Constraint::Length(3), // Header
                        Constraint::Length(5), // Active Window & Battery
                        Constraint::Length(6), // CPU & RAM Gauges
                        Constraint::Min(8),    // Interactive App List & Details
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
                Span::styled("v0.2.1", Style::default().fg(Color::DarkGray)),
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

            // Bottom Row Split: Interactive Top Apps (Left 55%) & Inspector Card (Right 45%)
            let bottom_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(55), Constraint::Percentage(45)].as_ref())
                .split(chunks[3]);

            let selected_idx = list_state.selected().unwrap_or(0);

            // Interactive Top Applications List (Left) displaying truncated name, RAM, and CPU with fixed alignment
            let app_items: Vec<ListItem> = visible_apps
                .iter()
                .enumerate()
                .map(|(idx, app)| {
                    let is_sel = idx == selected_idx;
                    let num_style = if is_sel {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };

                    let name_style = if is_sel {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                    };

                    let mem_style = if is_sel {
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Cyan)
                    };

                    let cpu_style = if is_sel {
                        Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Yellow)
                    };

                    let clean_display_name = truncate_name(&app.name, 18);

                    ListItem::new(Line::from(vec![
                        Span::styled(format!(" #{:<2} ", idx + 1), num_style),
                        Span::styled(format!("{:<18}", clean_display_name), name_style),
                        Span::styled(format!("{:>5} MB", app.ram_mb), mem_style),
                        Span::raw(" | "),
                        Span::styled(format!("{:>5.1}% CPU", app.cpu_pct), cpu_style),
                    ]))
                })
                .collect();

            let list_title = format!(
                " 📊 Applications (#{}/{} | Sort: {} {} [s/o] | Limit: {} [t]) ",
                selected_idx + 1,
                visible_count,
                sort_metric.label(),
                sort_order.symbol(),
                limit_mode.label()
            );

            let apps_list = List::new(app_items)
                .block(Block::default().title(list_title).borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)))
                .highlight_style(
                    Style::default()
                        .bg(Color::Rgb(35, 45, 65))
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▶ ");

            f.render_stateful_widget(apps_list, bottom_chunks[0], &mut list_state);

            // App Inspector Card (Right)
            let inspector_lines = if let Some(selected_app) = visible_apps.get(selected_idx) {
                vec![
                    Line::from(vec![
                        Span::styled("App Name: ", Style::default().fg(Color::Gray)),
                        Span::styled(&selected_app.name, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    ]),
                    Line::from(vec![
                        Span::styled("Path: ", Style::default().fg(Color::Gray)),
                        Span::styled(&selected_app.exe_path, Style::default().fg(Color::DarkGray)),
                    ]),
                    Line::from(vec![
                        Span::styled("Instances: ", Style::default().fg(Color::Gray)),
                        Span::styled(format!("{} processes", selected_app.process_count), Style::default().fg(Color::Magenta)),
                    ]),
                    Line::from(vec![
                        Span::styled("Physical RAM: ", Style::default().fg(Color::Gray)),
                        Span::styled(format!("{} MB ({:.1}% of System RAM)", selected_app.ram_mb, selected_app.ram_pct), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    ]),
                    Line::from(vec![
                        Span::styled("App CPU Load: ", Style::default().fg(Color::Gray)),
                        Span::styled(format!("{:.1}%", selected_app.cpu_pct), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    ]),
                ]
            } else {
                vec![Line::from(Span::raw("No process selected"))]
            };

            let inspector_widget = Paragraph::new(inspector_lines)
                .block(Block::default().title(" 🔍 Process Inspector ").borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)))
                .wrap(Wrap { trim: true });
            f.render_widget(inspector_widget, bottom_chunks[1]);

            // Footer Controls
            let pause_action_str = if is_paused { "resume" } else { "pause" };
            let footer = Paragraph::new(Line::from(vec![
                Span::styled(" Keys: ", Style::default().fg(Color::DarkGray)),
                Span::styled("↑/↓", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(" nav | ", Style::default().fg(Color::DarkGray)),
                Span::styled("s", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" sort ({}) | ", sort_metric.label()), Style::default().fg(Color::DarkGray)),
                Span::styled("o", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" order ({}) | ", sort_order.symbol()), Style::default().fg(Color::DarkGray)),
                Span::styled("t", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" limit ({}) | ", limit_mode.label()), Style::default().fg(Color::DarkGray)),
                Span::styled("p", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" {} | ", pause_action_str), Style::default().fg(Color::DarkGray)),
                Span::styled("q", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(" or ", Style::default().fg(Color::DarkGray)),
                Span::styled("Esc", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(" exit", Style::default().fg(Color::DarkGray)),
            ]));
            f.render_widget(footer, chunks[4]);
        })?;

        // Poll for keypress with 1s timeout
        if event::poll(Duration::from_secs(1))? {
            if let Event::Key(key) = event::read()? {
                let current_sel = list_state.selected().unwrap_or(0);
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
                    KeyCode::Char('s') | KeyCode::Char('S') => {
                        sort_metric = sort_metric.next();
                    }
                    KeyCode::Char('o') | KeyCode::Char('O') => {
                        sort_order = sort_order.toggle();
                    }
                    KeyCode::Char('t') | KeyCode::Char('T') => {
                        limit_mode = limit_mode.next();
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if visible_count > 0 {
                            let next = if current_sel + 1 >= visible_count { 0 } else { current_sel + 1 };
                            list_state.select(Some(next));
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if visible_count > 0 {
                            let prev = if current_sel == 0 { visible_count - 1 } else { current_sel - 1 };
                            list_state.select(Some(prev));
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
