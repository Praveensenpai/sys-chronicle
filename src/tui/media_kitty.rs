use base64::Engine;
use ratatui::layout::Rect;
use std::io::Write;
use std::path::Path;

pub fn is_kitty_terminal() -> bool {
    if let Ok(force) = std::env::var("FORCE_KITTY_GRAPHICS") {
        if force == "1" || force.eq_ignore_ascii_case("true") {
            return true;
        }
    }
    if std::env::var("KITTY_PID").is_ok() || std::env::var("KITTY_WINDOW_ID").is_ok() {
        return true;
    }
    if let Ok(term) = std::env::var("TERM") {
        if term.contains("kitty") || term.contains("ghostty") || term.contains("wezterm") {
            return true;
        }
    }
    if let Ok(prog) = std::env::var("TERM_PROGRAM") {
        if prog.eq_ignore_ascii_case("kitty")
            || prog.eq_ignore_ascii_case("ghostty")
            || prog.eq_ignore_ascii_case("wezterm")
        {
            return true;
        }
    }
    if let Some((app, _)) = crate::monitor::WindowMonitor::get_current_window() {
        if app.to_lowercase().contains("kitty") {
            return true;
        }
    }
    false
}

pub fn emit_raw_escape(seq: &str, in_tmux: bool) {
    let mut stdout = std::io::stdout().lock();
    if in_tmux {
        let escaped = seq.replace('\x1b', "\x1b\x1b");
        let _ = write!(stdout, "\x1bPtmux;\x1b{}\x1b\\", escaped);
    } else {
        let _ = write!(stdout, "{}", seq);
    }
    let _ = stdout.flush();
}

pub fn clear_kitty_graphics() {
    let in_tmux = std::env::var("TMUX").is_ok();
    emit_raw_escape("\x1b_Ga=d,d=a\x1b\\", in_tmux);
}

pub fn transmit_kitty_image(path: &Path, area: Rect) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let img = image::open(path)?;
    let target_w = ((area.width as u32) * 16).clamp(160, 640);
    let target_h = ((area.height as u32) * 32).clamp(160, 800);
    let resized = img.resize_exact(target_w, target_h, image::imageops::FilterType::Lanczos3);

    let mut png_bytes = Vec::new();
    resized.write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)?;

    let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
    let in_tmux = std::env::var("TMUX").is_ok();

    let move_cmd = format!("\x1b[{};{}H", area.y + 1, area.x + 1);
    emit_raw_escape(&move_cmd, in_tmux);

    let chunk_size = 3000;
    let total_chunks = b64.len().div_ceil(chunk_size);

    for (i, chunk) in b64.as_bytes().chunks(chunk_size).enumerate() {
        let is_last = i + 1 == total_chunks;
        let m = if is_last { 0 } else { 1 };
        let chunk_str = std::str::from_utf8(chunk)?;

        let cmd = if i == 0 {
            format!(
                "\x1b_Ga=T,f=100,c={},r={},C=1,m={};{}\x1b\\",
                area.width, area.height, m, chunk_str
            )
        } else {
            format!("\x1b_Gm={};{}\x1b\\", m, chunk_str)
        };
        emit_raw_escape(&cmd, in_tmux);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_kitty_terminal_with_forced_flag() {
        std::env::set_var("FORCE_KITTY_GRAPHICS", "1");
        assert!(is_kitty_terminal());
        std::env::remove_var("FORCE_KITTY_GRAPHICS");
    }

    #[test]
    fn test_escape_sequence_structure() {
        let seq = "\x1b_Ga=d,d=a\x1b\\";
        let escaped = seq.replace('\x1b', "\x1b\x1b");
        assert_eq!(escaped, "\x1b\x1b_Ga=d,d=a\x1b\x1b\\");
    }
}
