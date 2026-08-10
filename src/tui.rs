use anyhow::Result;
use chrono::{DateTime, Local};
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};
use std::collections::BTreeMap;
use std::io::stdout;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use sysinfo::System;

use crate::exporter::generate_ai_report;
use crate::logger::{ActivityEvent, LogWriter};
use crate::monitor::metrics::AppDetail;
use crate::monitor::{MetricsMonitor, PowerMonitor, WindowMonitor};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveTab {
    Live,
    Analytics,
}

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

struct ScreenTimeEntry {
    app_name: String,
    total_seconds: i64,
}

fn compute_daily_screen_time() -> Vec<ScreenTimeEntry> {
    let today = Local::now().format("%Y-%m-%d").to_string();
    let events = match LogWriter::read_events_for_date(&today) {
        Ok(evs) => evs,
        Err(_) => return Vec::new(),
    };

    if events.is_empty() {
        return Vec::new();
    }

    let mut duration_map: BTreeMap<String, i64> = BTreeMap::new();
    let mut window_events: Vec<(DateTime<Local>, String)> = Vec::new();

    for event in events {
        if let ActivityEvent::WindowFocus { timestamp, app_class, duration_secs, .. } = event {
            if let Some(dur) = duration_secs {
                *duration_map.entry(app_class).or_insert(0) += dur as i64;
            } else if let Ok(dt) = DateTime::parse_from_rfc3339(&timestamp) {
                window_events.push((dt.with_timezone(&Local::now().timezone()), app_class));
            }
        }
    }

    for window in window_events.windows(2) {
        let (dt1, app1) = &window[0];
        let (dt2, _) = &window[1];
        let diff_secs = (*dt2 - *dt1).num_seconds();

        // Cap single session gaps at 10 minutes to avoid sleep/idle inflating numbers
        if diff_secs > 0 && diff_secs < 600 {
            *duration_map.entry(app1.clone()).or_insert(0) += diff_secs;
        }
    }

    let mut entries: Vec<ScreenTimeEntry> = duration_map
        .into_iter()
        .map(|(app_name, total_seconds)| ScreenTimeEntry { app_name, total_seconds })
        .collect();

    entries.sort_by_key(|e| std::cmp::Reverse(e.total_seconds));
    entries
}

pub fn run_status_tui() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut sys = System::new_all();

    let mut active_tab = ActiveTab::Live;
    let mut is_paused = false;
    let mut paused_at: Option<DateTime<Local>> = None;
    let mut limit_mode = LimitMode::Top10;
    let mut sort_metric = SortMetric::Ram;
    let mut sort_order = SortOrder::Descending;

    let mut search_active = false;
    let mut search_query = String::new();
    let mut kill_modal_target: Option<AppDetail> = None;
    let mut toast_message: Option<(String, Instant)> = None;

    let mut list_state = ListState::default();
    list_state.select(Some(0));

    // Cached frozen snapshot for paused state
    let mut cached_window = WindowMonitor::get_current_window();
    let mut cached_power = PowerMonitor::read_current_state();
    let mut cached_metrics = MetricsMonitor::sample_metrics(&mut sys);
    let mut cached_analytics = compute_daily_screen_time();

    loop {
        let now = Local::now();

        // Sample current data if not paused
        if !is_paused {
            cached_window = WindowMonitor::get_current_window();
            cached_power = PowerMonitor::read_current_state();
            cached_metrics = MetricsMonitor::sample_metrics(&mut sys);
            cached_analytics = compute_daily_screen_time();
        }

        let window_info = &cached_window;
        let power_info = &cached_power;
        let metrics = &cached_metrics;

        // Apply search query filter if search is active
        let mut sorted_apps: Vec<AppDetail> = metrics.app_details.clone();
        if !search_query.trim().is_empty() {
            let query = search_query.to_lowercase();
            sorted_apps.retain(|app| app.name.to_lowercase().contains(&query) || app.exe_path.to_lowercase().contains(&query));
        }

        // Apply sorting to app details
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
                        Constraint::Length(3), // Header & Tabs
                        Constraint::Length(5), // Active Window, Power & Sensors
                        Constraint::Length(6), // CPU & RAM Gauges
                        Constraint::Min(8),    // Interactive App List or Analytics Tab
                        Constraint::Length(1), // Footer controls
                    ]
                    .as_ref(),
                )
                .split(f.size());

            // Header Status Badge & Tab Bar
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

            let live_tab_style = if active_tab == ActiveTab::Live {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let analytics_tab_style = if active_tab == ActiveTab::Analytics {
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            // Toast feedback
            let toast_span = if let Some((ref msg, inst)) = toast_message {
                if inst.elapsed() < Duration::from_secs(3) {
                    Span::styled(format!(" | {} ", msg), Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD))
                } else {
                    Span::raw("")
                }
            } else {
                Span::raw("")
            };

            let header = Paragraph::new(Line::from(vec![
                Span::styled(" ⏱️  SysChronicle ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled("v0.3.3", Style::default().fg(Color::DarkGray)),
                Span::raw(" | "),
                status_span,
                Span::raw(" | Tab: ["),
                Span::styled("Live Dashboard (Tab)", live_tab_style),
                Span::raw("] ["),
                Span::styled("Daily Analytics (Tab)", analytics_tab_style),
                Span::raw("]"),
                toast_span,
            ]))
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)));
            f.render_widget(header, chunks[0]);

            // Top Row: Active Window, Power & Sensors
            let top_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(25), Constraint::Percentage(25)].as_ref())
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

            // Hardware sensor widget. Some systems do not expose hwmon data, so show a
            // clear unavailable state instead of treating it as an error.
            let temp_line = match metrics.cpu_temp_c {
                Some(temp) => Line::from(vec![
                    Span::styled("CPU Temp: ", Style::default().fg(Color::Gray)),
                    Span::styled(format!("{temp:.1}°C"), Style::default().fg(if temp >= 90.0 { Color::Red } else if temp >= 75.0 { Color::Yellow } else { Color::Green }).add_modifier(Modifier::BOLD)),
                    Span::styled(
                        format!(" ({})", metrics.cpu_temp_label.as_deref().unwrap_or("CPU sensor")),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
                None => Line::from(vec![
                    Span::styled("CPU Temp: ", Style::default().fg(Color::Gray)),
                    Span::styled("Unavailable", Style::default().fg(Color::DarkGray)),
                ]),
            };
            let fan_line = match metrics.fan_rpm {
                Some(rpm) => Line::from(vec![
                    Span::styled("Fan Speed: ", Style::default().fg(Color::Gray)),
                    Span::styled(format!("{rpm} RPM"), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::styled(
                        format!(" ({})", metrics.fan_label.as_deref().unwrap_or("fan sensor")),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
                None => Line::from(vec![
                    Span::styled("Fan Speed: ", Style::default().fg(Color::Gray)),
                    Span::styled("Unavailable", Style::default().fg(Color::DarkGray)),
                ]),
            };
            let sensors_widget = Paragraph::new(vec![temp_line, fan_line])
                .block(Block::default().title(" 🌡️ Sensors ").borders(Borders::ALL).border_style(Style::default().fg(Color::LightRed)));
            f.render_widget(sensors_widget, top_chunks[2]);

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

            // Main Content Area based on Active Tab
            match active_tab {
                ActiveTab::Live => {
                    let bottom_chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)].as_ref())
                        .split(chunks[3]);

                    let selected_idx = list_state.selected().unwrap_or(0);

                    // Interactive Top Applications List (Left) displaying truncated name, RAM, and CPU
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

                    let search_str = if search_active || !search_query.is_empty() {
                        format!(" | 🔍 Query: '{}'", search_query)
                    } else {
                        String::new()
                    };

                    let list_title = format!(
                        " 📊 Applications (#{}/{} | Sort: {} {} [s/o] | Limit: {} [t]{}) ",
                        if visible_count > 0 { selected_idx + 1 } else { 0 },
                        visible_count,
                        sort_metric.label(),
                        sort_order.symbol(),
                        limit_mode.label(),
                        search_str
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
                        vec![Line::from(Span::raw("No process matches filter"))]
                    };

                    let inspector_widget = Paragraph::new(inspector_lines)
                        .block(Block::default().title(" 🔍 Process Inspector ").borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)))
                        .wrap(Wrap { trim: true });
                    f.render_widget(inspector_widget, bottom_chunks[1]);
                }
                ActiveTab::Analytics => {
                    let analytics_items: Vec<ListItem> = if cached_analytics.is_empty() {
                        vec![ListItem::new(Span::styled("  No screen time logged yet for today", Style::default().fg(Color::DarkGray)))]
                    } else {
                        cached_analytics
                            .iter()
                            .enumerate()
                            .map(|(idx, entry)| {
                                let formatted_time = format_screen_time(entry.total_seconds);
                                ListItem::new(Line::from(vec![
                                    Span::styled(format!(" #{:<2} ", idx + 1), Style::default().fg(Color::DarkGray)),
                                    Span::styled(format!("{:<25}", entry.app_name), Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                                    Span::styled(formatted_time, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                                ]))
                            })
                            .collect()
                    };

                    let analytics_list = List::new(analytics_items)
                        .block(Block::default().title(" 📈 Today's Accumulated Active Window Screen Time ").borders(Borders::ALL).border_style(Style::default().fg(Color::Magenta)));

                    f.render_widget(analytics_list, chunks[3]);
                }
            }

            // Footer Controls
            let pause_action_str = if is_paused { "resume" } else { "pause" };
            let footer_lines = vec![
                Span::styled(" Keys: ", Style::default().fg(Color::DarkGray)),
                Span::styled("Tab", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled(" view | ", Style::default().fg(Color::DarkGray)),
                Span::styled("/", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(" search | ", Style::default().fg(Color::DarkGray)),
                Span::styled("K", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled(" kill | ", Style::default().fg(Color::DarkGray)),
                Span::styled("e", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(" AI export | ", Style::default().fg(Color::DarkGray)),
                Span::styled("s", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" sort ({}) | ", sort_metric.label()), Style::default().fg(Color::DarkGray)),
                Span::styled("o", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" order ({}) | ", sort_order.symbol()), Style::default().fg(Color::DarkGray)),
                Span::styled("t", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" limit ({}) | ", limit_mode.label()), Style::default().fg(Color::DarkGray)),
                Span::styled("p", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" {} | ", pause_action_str), Style::default().fg(Color::DarkGray)),
                Span::styled("q", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(" exit", Style::default().fg(Color::DarkGray)),
            ];
            let footer = Paragraph::new(Line::from(footer_lines));
            f.render_widget(footer, chunks[4]);

            // Render Kill Process Pop-up Confirmation Modal
            if let Some(target) = &kill_modal_target {
                let area = centered_rect(50, 25, f.size());
                f.render_widget(Clear, area);

                let modal_text = vec![
                    Line::from(vec![
                        Span::styled("⚡ Terminate Application ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                        Span::styled(format!("'{}'", target.name), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                        Span::raw("?"),
                    ]),
                    Line::from(vec![
                        Span::styled(format!("Instances: {} processes | RAM: {} MB", target.process_count, target.ram_mb), Style::default().fg(Color::Gray)),
                    ]),
                    Line::from(Span::styled("This will send SIGTERM/kill signals to these processes.", Style::default().fg(Color::DarkGray))),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled(" Press ", Style::default().fg(Color::Gray)),
                        Span::styled("y", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                        Span::styled(" or ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                        Span::styled("Y", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                        Span::styled(" to confirm | ", Style::default().fg(Color::Gray)),
                        Span::styled("n", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                        Span::styled(" / ", Style::default().fg(Color::Gray)),
                        Span::styled("Esc", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                        Span::styled(" to cancel", Style::default().fg(Color::Gray)),
                    ]),
                ];

                let modal_block = Paragraph::new(modal_text)
                    .alignment(Alignment::Center)
                    .block(Block::default().title(" ⚠️ Confirm Process Termination ").borders(Borders::ALL).border_style(Style::default().fg(Color::Red)));

                f.render_widget(modal_block, area);
            }
        })?;

        // Poll for keypress with 1s timeout
        if event::poll(Duration::from_secs(1))? {
            if let Event::Key(key) = event::read()? {
                // If Kill Modal is active, intercept keypresses
                if let Some(target) = kill_modal_target.clone() {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            let terminated_count = kill_app_processes(&mut sys, &target);
                            toast_message = Some((format!("✔ Terminated {} instance(s) of '{}'", terminated_count, target.name), Instant::now()));
                            kill_modal_target = None;
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                            kill_modal_target = None;
                        }
                        _ => {}
                    }
                    continue;
                }

                // If Search Mode is active, capture input characters
                if search_active {
                    match key.code {
                        KeyCode::Esc | KeyCode::Enter => {
                            search_active = false;
                        }
                        KeyCode::Backspace => {
                            search_query.pop();
                        }
                        KeyCode::Char(c) => {
                            search_query.push(c);
                        }
                        _ => {}
                    }
                    continue;
                }

                // Standard Key Dispatcher
                let current_sel = list_state.selected().unwrap_or(0);
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Tab => {
                        active_tab = match active_tab {
                            ActiveTab::Live => ActiveTab::Analytics,
                            ActiveTab::Analytics => ActiveTab::Live,
                        };
                    }
                    KeyCode::Char('/') => {
                        search_active = true;
                    }
                    KeyCode::Char('K') => {
                        if let Some(selected_app) = visible_apps.get(current_sel) {
                            kill_modal_target = Some(selected_app.clone());
                        }
                    }
                    KeyCode::Char('e') | KeyCode::Char('E') => {
                        let title_date = Local::now().format("%Y-%m-%d").to_string();
                        if let Ok(events) = LogWriter::read_events_for_date(&title_date) {
                            let report = generate_ai_report(&events, &title_date);
                            if copy_to_clipboard(&report).is_ok() {
                                toast_message = Some(("✔ AI payload copied to clipboard!".to_string(), Instant::now()));
                            } else {
                                toast_message = Some(("❌ Failed to run wl-copy".to_string(), Instant::now()));
                            }
                        }
                    }
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

fn kill_app_processes(sys: &mut System, target: &AppDetail) -> usize {
    sys.refresh_processes_specifics(sysinfo::ProcessRefreshKind::everything());
    let mut killed = 0;

    for (_pid, p) in sys.processes() {
        let clean_name = p
            .exe()
            .and_then(|e| e.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| p.name().to_string());

        let raw_exe_path = p.exe().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();

        if clean_name == target.name || (!target.exe_path.is_empty() && raw_exe_path == target.exe_path) {
            if p.kill() {
                killed += 1;
            }
        }
    }

    killed
}

fn copy_to_clipboard(text: &str) -> Result<()> {
    let mut child = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(text.as_bytes())?;
    }

    child.wait()?;
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

fn format_screen_time(secs: i64) -> String {
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;

    if hours > 0 {
        format!("{}h {}m {}s", hours, mins, s)
    } else if mins > 0 {
        format!("{}m {}s", mins, s)
    } else {
        format!("{}s", s)
    }
}
