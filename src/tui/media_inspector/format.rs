use regex::Regex;

pub fn format_time(secs: u64) -> String {
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, mins, s)
    } else {
        format!("{:02}:{:02}", mins, s)
    }
}

pub fn clean_display_title(s: &str) -> String {
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

pub fn format_display_folder(path: &str, max_len: usize) -> Option<String> {
    let p = std::path::Path::new(path);
    let parent = p.parent()?;
    let parent_str = parent.to_str()?;
    if parent_str.is_empty() || parent_str == "/" || parent_str == "." {
        return None;
    }
    if parent_str.chars().count() <= max_len {
        return Some(parent_str.to_string());
    }
    let last_dir = parent.file_name().and_then(|f| f.to_str())?;
    if let Some(grandparent) = parent
        .parent()
        .and_then(|gp| gp.file_name())
        .and_then(|f| f.to_str())
    {
        let candidate = format!(".../{}/{}", grandparent, last_dir);
        if candidate.chars().count() <= max_len {
            return Some(candidate);
        }
    }
    let candidate = format!(".../{}", last_dir);
    if candidate.chars().count() <= max_len {
        return Some(candidate);
    }
    let char_count = last_dir.chars().count();
    let take_n = max_len.saturating_sub(3).max(1);
    let end: String = last_dir
        .chars()
        .skip(char_count.saturating_sub(take_n))
        .collect();
    Some(format!("...{}", end))
}

pub fn format_display_filename(path: &str, max_len: usize) -> String {
    let p = std::path::Path::new(path);
    let file_name = p.file_name().and_then(|f| f.to_str()).unwrap_or(path);
    if file_name.chars().count() <= max_len {
        return file_name.to_string();
    }
    let limit = max_len.saturating_sub(1);
    let truncated: String = file_name.chars().take(limit).collect();
    format!("{truncated}…")
}

pub fn truncate_footer_message(msg: &str, max_width: usize) -> String {
    let limit = max_width.saturating_sub(1);
    if limit == 0 {
        return String::new();
    }
    if msg.chars().count() > limit {
        let truncated: String = msg.chars().take(limit.saturating_sub(1)).collect();
        format!("{truncated}…")
    } else {
        msg.to_string()
    }
}
