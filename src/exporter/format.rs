//! Pure formatting helpers shared by the report sections.

/// Reduces an RFC 3339 timestamp to its clock component.
///
/// Input format: 2026-08-10T00:21:24.123+05:30 -> 00:21:24
pub fn extract_short_time(ts: &str) -> String {
    if let Some(time_part) = ts.split('T').nth(1) {
        let clean = time_part.split('+').next().unwrap_or(time_part);
        clean.split('.').next().unwrap_or(clean).to_string()
    } else {
        ts.to_string()
    }
}

/// Renders a duration in a compact human readable form.
pub fn format_duration(secs: u64) -> String {
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;

    if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else if mins > 0 {
        format!("{}m {}s", mins, s)
    } else {
        format!("{}s", s)
    }
}

/// Strips the memory annotation from a process label.
pub fn top_app_name(top_app: &str) -> String {
    top_app
        .split_once(" (")
        .map(|(name, _)| name)
        .unwrap_or(top_app)
        .trim()
        .to_string()
}

/// Maps internal process names to reader friendly labels.
pub fn display_app_name(app: &str) -> String {
    match app {
        "jetbrains-studio" | "studio" => format!("Android Studio ({})", app),
        _ => app.to_string(),
    }
}

/// Removes well known application suffixes from a window title.
pub fn clean_window_title(title: &str, _app_class: &str) -> String {
    let t = title.trim();
    if t.is_empty() {
        return String::new();
    }

    let suffixes = [
        " - mpv",
        " - Google Chrome",
        " - Visual Studio Code",
        " — Mozilla Firefox",
        " - Alacritty",
    ];

    let mut cleaned = t;
    for suffix in suffixes {
        if cleaned.ends_with(suffix) {
            cleaned = &cleaned[..cleaned.len() - suffix.len()];
        }
    }

    cleaned.trim().to_string()
}
