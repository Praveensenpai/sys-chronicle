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

pub fn compute_portrait_cells(
    orig_w: u32,
    orig_h: u32,
    area_w: u16,
    area_h: u16,
) -> (u16, u16, u16, u16) {
    if area_w == 0 || area_h == 0 || orig_w == 0 || orig_h == 0 {
        return (0, 0, 0, 0);
    }

    let img_ratio = orig_w as f32 / orig_h as f32;
    let ideal_cols = (area_h as f32 * 2.0 * img_ratio).round() as u16;

    let (disp_cols, disp_rows) = if ideal_cols <= area_w {
        (ideal_cols.max(2), area_h)
    } else {
        let ideal_rows = (area_w as f32 / (2.0 * img_ratio)).round() as u16;
        (area_w, ideal_rows.min(area_h).max(2))
    };

    let col_offset = area_w.saturating_sub(disp_cols) / 2;
    let row_offset = area_h.saturating_sub(disp_rows) / 2;

    (disp_cols, disp_rows, col_offset, row_offset)
}

pub fn transmit_kitty_image(
    path: &Path,
    area: Rect,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let img = image::open(path)?;
    let (orig_w, orig_h) = (img.width(), img.height());

    let (disp_cols, disp_rows, col_offset, row_offset) =
        compute_portrait_cells(orig_w, orig_h, area.width, area.height);

    if disp_cols == 0 || disp_rows == 0 {
        return Ok(());
    }

    let target_w = (disp_cols as u32 * 24).clamp(160, 720);
    let target_h = (disp_rows as u32 * 48).clamp(240, 960);
    let resized = img.resize(target_w, target_h, image::imageops::FilterType::Lanczos3);

    let mut png_bytes = Vec::new();
    resized.write_to(
        &mut std::io::Cursor::new(&mut png_bytes),
        image::ImageFormat::Png,
    )?;

    let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
    let in_tmux = std::env::var("TMUX").is_ok();

    let move_cmd = format!(
        "\x1b[{};{}H",
        area.y + 1 + row_offset,
        area.x + 1 + col_offset
    );
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
                disp_cols, disp_rows, m, chunk_str
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

    #[test]
    fn test_compute_portrait_cells_preserves_aspect_ratio() {
        // Typical MAL anime portrait cover: 225x318 (ratio ~0.7075)
        let (cols, rows, col_off, row_off) = compute_portrait_cells(225, 318, 20, 11);
        assert_eq!(cols, 16);
        assert_eq!(rows, 11);
        assert_eq!(col_off, 2);
        assert_eq!(row_off, 0);

        // Wide box 24 cols x 8 rows
        let (cols, rows, col_off, row_off) = compute_portrait_cells(225, 318, 24, 8);
        assert_eq!(cols, 11);
        assert_eq!(rows, 8);
        assert_eq!(col_off, 6);
        assert_eq!(row_off, 0);
    }
}
