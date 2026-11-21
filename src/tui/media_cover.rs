use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::media_kitty::{
    clear_kitty_graphics, compute_portrait_cells, is_kitty_terminal, transmit_kitty_image,
};
use crate::mal::history::AnimeSyncRecord;

/// In-memory cache for rendered cover graphics (Kitty protocol or Unicode half-blocks).
pub struct CoverCache {
    rendered: HashMap<(String, u16, u16), Vec<Line<'static>>>,
    in_flight: Arc<Mutex<HashSet<String>>>,
    last_rendered_key: Option<String>,
}

impl CoverCache {
    pub fn new() -> Self {
        Self {
            rendered: HashMap::new(),
            in_flight: Arc::new(Mutex::new(HashSet::new())),
            last_rendered_key: None,
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, record: &AnimeSyncRecord) {
        if area.width < 4 || area.height < 4 {
            return;
        }

        let disk_path = cover_cache_path(record);
        let local_path = record
            .path
            .as_deref()
            .or(Some(record.raw_title.as_str()))
            .and_then(find_local_cover);

        let active_path = if let Some(local) = local_path {
            Some(local)
        } else if disk_path.exists() {
            Some(disk_path.clone())
        } else {
            None
        };

        if is_kitty_terminal() {
            if let Some(ref path) = active_path {
                self.render_kitty_cover(f, area, path);
                return;
            }
            clear_kitty_graphics();
            self.last_rendered_key = None;
        } else if let Some(ref path) = active_path {
            if self.render_halfblock_cover(f, area, path) {
                return;
            }
        }

        let is_fetching = trigger_background_fetch(record, disk_path, &self.in_flight);
        render_placeholder(f, area, is_fetching);
    }

    fn render_kitty_cover(&mut self, f: &mut Frame, area: Rect, path: &Path) {
        let key = format!("{}:{}:{}", path.display(), area.width, area.height);
        if self.last_rendered_key.as_deref() != Some(&key) {
            clear_kitty_graphics();
            if transmit_kitty_image(path, area).is_ok() {
                self.last_rendered_key = Some(key);
            }
        }

        let blank_str = " ".repeat(area.width as usize);
        let blank_lines = vec![Line::from(blank_str); area.height as usize];
        f.render_widget(Paragraph::new(blank_lines), area);
    }

    fn render_halfblock_cover(&mut self, f: &mut Frame, area: Rect, path: &Path) -> bool {
        let key = (path.to_string_lossy().to_string(), area.width, area.height);
        if !self.rendered.contains_key(&key) {
            if let Some(lines) = decode_halfblocks(path, area.width, area.height) {
                self.rendered.insert(key.clone(), lines);
            }
        }

        if let Some(lines) = self.rendered.get(&key) {
            let p = Paragraph::new(lines.clone()).alignment(Alignment::Center);
            f.render_widget(p, area);
            true
        } else {
            false
        }
    }
}

impl Default for CoverCache {
    fn default() -> Self {
        Self::new()
    }
}

fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("~/.cache"))
        .join("sys-chronicle")
        .join("covers")
}

fn cover_cache_path(record: &AnimeSyncRecord) -> PathBuf {
    let filename = if let Some(id) = record.mal_anime_id {
        format!("mal_{}.jpg", id)
    } else {
        let mut hasher = Sha256::new();
        hasher.update(record.canonical_title.as_bytes());
        format!("title_{:x}.jpg", hasher.finalize())
    };
    cache_dir().join(filename)
}

fn find_local_cover(media_path: &str) -> Option<PathBuf> {
    let p = Path::new(media_path);
    let dir = if p.is_dir() { p } else { p.parent()? };

    let candidates = [
        "cover.jpg",
        "cover.png",
        "cover.webp",
        "poster.jpg",
        "poster.png",
        "folder.jpg",
        "folder.png",
    ];

    for name in candidates {
        let file = dir.join(name);
        if file.exists() {
            return Some(file);
        }
    }
    None
}

fn trigger_background_fetch(
    record: &AnimeSyncRecord,
    target_path: PathBuf,
    in_flight: &Arc<Mutex<HashSet<String>>>,
) -> bool {
    let key = record
        .mal_image_url
        .clone()
        .or_else(|| record.mal_anime_id.map(|id| format!("id_{}", id)))
        .unwrap_or_else(|| format!("title_{}", record.canonical_title));

    let mut set = match in_flight.lock() {
        Ok(guard) => guard,
        Err(_) => return false,
    };

    if set.contains(&key) {
        return true;
    }
    set.insert(key.clone());
    drop(set);

    let in_flight_clone = Arc::clone(in_flight);
    let key_clone = key;
    let url_opt = record.mal_image_url.clone();
    let mal_id_opt = record.mal_anime_id;
    let title_clean = record.canonical_title.clone();

    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async move {
            let _ = fetch_cover_worker(url_opt, mal_id_opt, &title_clean, target_path).await;
            if let Ok(mut set) = in_flight_clone.lock() {
                set.remove(&key_clone);
            }
        });
    } else {
        std::thread::spawn(move || {
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                rt.block_on(async move {
                    let _ =
                        fetch_cover_worker(url_opt, mal_id_opt, &title_clean, target_path).await;
                });
            }
            if let Ok(mut set) = in_flight_clone.lock() {
                set.remove(&key_clone);
            }
        });
    }

    true
}

async fn fetch_cover_worker(
    url_opt: Option<String>,
    mal_id_opt: Option<u64>,
    title: &str,
    target_path: PathBuf,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let image_url = if let Some(u) = url_opt {
        u
    } else {
        let config = crate::mal::MalAuth::load_config().ok();
        let token = if let Some(mut cfg) = config {
            crate::mal::MalAuth::get_valid_token(&mut cfg).await.ok()
        } else {
            None
        };

        if let Some(tok) = token {
            let client = crate::mal::MalClient::new(tok)?;
            if let Some(id) = mal_id_opt {
                let node = client.get_anime(id).await?;
                node.main_picture
                    .and_then(|p| p.best_url().map(String::from))
                    .ok_or("no picture")?
            } else {
                let query = crate::mal::candidate::seed_query_from_title(title);
                let nodes = client.search_anime_limited(&query, 3).await?;
                let best = nodes.into_iter().next().ok_or("no results")?;
                best.main_picture
                    .and_then(|p| p.best_url().map(String::from))
                    .ok_or("no picture")?
            }
        } else {
            return Err("no auth token".into());
        }
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let bytes = client.get(&image_url).send().await?.bytes().await?;

    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&target_path, bytes)?;
    Ok(())
}

fn decode_halfblocks(path: &Path, width: u16, height: u16) -> Option<Vec<Line<'static>>> {
    let img = image::open(path).ok()?.to_rgb8();
    let (orig_w, orig_h) = (img.width(), img.height());
    let (disp_cols, disp_rows, col_offset, row_offset) =
        compute_portrait_cells(orig_w, orig_h, width, height);

    if disp_cols == 0 || disp_rows == 0 {
        return None;
    }

    let target_w = disp_cols as u32;
    let target_h = (disp_rows as u32) * 2;
    let resized = image::imageops::resize(
        &img,
        target_w,
        target_h,
        image::imageops::FilterType::Triangle,
    );

    let mut lines = Vec::with_capacity(height as usize);
    for _ in 0..row_offset {
        lines.push(Line::from(" ".repeat(width as usize)));
    }

    let right_pad = (width as usize).saturating_sub(disp_cols as usize + col_offset as usize);
    let left_pad_str = " ".repeat(col_offset as usize);
    let right_pad_str = " ".repeat(right_pad);

    for y in 0..disp_rows {
        let mut spans = Vec::with_capacity(disp_cols as usize + 2);
        if col_offset > 0 {
            spans.push(Span::raw(left_pad_str.clone()));
        }

        let top_y = (y as u32) * 2;
        let bot_y = top_y + 1;

        for x in 0..disp_cols {
            let top_p = resized.get_pixel(x as u32, top_y);
            let bot_p = if bot_y < target_h {
                resized.get_pixel(x as u32, bot_y)
            } else {
                top_p
            };

            let fg = Color::Rgb(top_p[0], top_p[1], top_p[2]);
            let bg = Color::Rgb(bot_p[0], bot_p[1], bot_p[2]);

            spans.push(Span::styled("▀", Style::default().fg(fg).bg(bg)));
        }

        if right_pad > 0 {
            spans.push(Span::raw(right_pad_str.clone()));
        }
        lines.push(Line::from(spans));
    }

    let remaining_rows = (height as usize).saturating_sub(lines.len());
    for _ in 0..remaining_rows {
        lines.push(Line::from(" ".repeat(width as usize)));
    }

    Some(lines)
}

fn render_placeholder(f: &mut Frame, area: Rect, is_fetching: bool) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let label = if is_fetching {
        "Fetching Cover"
    } else {
        "No Cover Art"
    };

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "┌── ✦ ──┐",
            Style::default().fg(Color::Rgb(80, 80, 110)),
        )),
        Line::from(Span::styled(
            label,
            Style::default()
                .fg(Color::Rgb(140, 140, 170))
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "└── ✦ ──┘",
            Style::default().fg(Color::Rgb(80, 80, 110)),
        )),
    ];

    let p = Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Center);
    f.render_widget(p, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_halfblocks_produces_correct_dimensions_and_colors() {
        let temp_dir = std::env::temp_dir();
        let img_path = temp_dir.join("test_cover_sample.png");

        let mut img = image::RgbImage::new(4, 4);
        for x in 0..4 {
            for y in 0..4 {
                img.put_pixel(x, y, image::Rgb([255, 0, 0]));
            }
        }
        img.save(&img_path).expect("save test image");

        let lines = decode_halfblocks(&img_path, 4, 2).expect("decoded");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].spans.len(), 4);
        assert_eq!(lines[0].spans[0].content, "▀");
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Rgb(255, 0, 0)));
        assert_eq!(lines[0].spans[0].style.bg, Some(Color::Rgb(255, 0, 0)));

        let _ = std::fs::remove_file(img_path);
    }
}
