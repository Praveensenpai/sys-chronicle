use anyhow::{Context, Result};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use crate::monitor::PlaybackInterval;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SyncStatus {
    Watching,
    ThresholdMet,
    Synced,
    AheadOnMal { mal_episode: u32 },
    Failed { error: String },
}

impl SyncStatus {
    pub fn badge(&self) -> (&'static str, ratatui::style::Color) {
        match self {
            SyncStatus::Synced => ("✔ SYNCED", ratatui::style::Color::Green),
            SyncStatus::AheadOnMal { .. } => ("⏩ MAL AHEAD", ratatui::style::Color::Cyan),
            SyncStatus::ThresholdMet => ("⏳ PENDING", ratatui::style::Color::Yellow),
            SyncStatus::Watching => ("▶ WATCHING", ratatui::style::Color::Blue),
            SyncStatus::Failed { .. } => ("✖ FAILED", ratatui::style::Color::Red),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimeSyncRecord {
    pub raw_title: String,
    pub canonical_title: String,
    pub episode: u32,
    pub path: Option<String>,
    pub duration_secs: u64,
    pub watched_secs: u64,
    #[serde(default)]
    pub position_secs: Option<u64>,
    pub watch_pct: f32,
    pub mal_anime_id: Option<u64>,
    pub mal_current_ep: Option<u32>,
    pub mal_total_episodes: Option<u32>,
    pub status: SyncStatus,
    pub last_updated: String,
    #[serde(default)]
    pub intervals: Vec<PlaybackInterval>,
}

#[derive(Debug, Clone)]
pub struct ProgressUpdateParams<'a> {
    pub raw_title: &'a str,
    pub canonical_title: &'a str,
    pub episode: u32,
    pub path: Option<String>,
    pub duration_secs: u64,
    pub watched_secs: u64,
    pub position_secs: Option<u64>,
    pub intervals: &'a [PlaybackInterval],
}

pub struct AnimeSyncHistory;

impl AnimeSyncHistory {
    pub fn storage_path() -> PathBuf {
        #[cfg(test)]
        {
            std::env::temp_dir().join("sys_chronicle_test_anime_sync_history.json")
        }
        #[cfg(not(test))]
        {
            dirs::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("~/.local/share"))
                .join("sys-chronicle")
                .join("anime_sync_history.json")
        }
    }

    pub fn load_all() -> Result<Vec<AnimeSyncRecord>> {
        let path = Self::storage_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read history from {:?}", path))?;
        if content.trim().is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse JSON history from {:?}", path))
    }

    pub fn save_all(records: &[AnimeSyncRecord]) -> Result<()> {
        let path = Self::storage_path();
        if let Some(parent) = path.parent() {
            create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(records)?;
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;
        file.write_all(content.as_bytes())?;
        Ok(())
    }

    pub fn upsert(record: AnimeSyncRecord) -> Result<()> {
        let mut records = Self::load_all().unwrap_or_default();
        if let Some(idx) = records.iter().position(|r| {
            (r.canonical_title
                .eq_ignore_ascii_case(&record.canonical_title)
                && r.episode == record.episode)
                || r.raw_title == record.raw_title
        }) {
            records[idx] = record;
        } else {
            records.insert(0, record);
        }
        Self::save_all(&records)
    }

    pub fn find_record(
        raw_title: &str,
        canonical_title: &str,
        episode: u32,
    ) -> Option<AnimeSyncRecord> {
        let records = Self::load_all().ok()?;
        records.into_iter().find(|r| {
            (r.canonical_title.eq_ignore_ascii_case(canonical_title) && r.episode == episode)
                || r.raw_title == raw_title
        })
    }

    pub fn update_progress_full(params: ProgressUpdateParams<'_>) -> Result<()> {
        let mut records = Self::load_all().unwrap_or_default();
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        if let Some(r) = records.iter_mut().find(|r| {
            (r.canonical_title
                .eq_ignore_ascii_case(params.canonical_title)
                && r.episode == params.episode)
                || r.raw_title == params.raw_title
        }) {
            // Anti-cheat: watched_secs can NEVER decrease upon seek or rewatch
            r.watched_secs = r.watched_secs.max(params.watched_secs);
            r.duration_secs = params.duration_secs;
            r.position_secs = params.position_secs.or(r.position_secs);
            r.watch_pct = if params.duration_secs > 0 {
                (r.watched_secs as f32 / params.duration_secs as f32 * 100.0).min(100.0)
            } else {
                0.0
            };
            if !params.intervals.is_empty() {
                r.intervals = params.intervals.to_vec();
            }
            r.last_updated = now;
            if r.status == SyncStatus::Watching && r.watch_pct >= 80.0 {
                r.status = SyncStatus::ThresholdMet;
            }
        } else {
            let actual_watched = params.watched_secs;
            let watch_pct = if params.duration_secs > 0 {
                (actual_watched as f32 / params.duration_secs as f32 * 100.0).min(100.0)
            } else {
                0.0
            };
            let status = if watch_pct >= 80.0 {
                SyncStatus::ThresholdMet
            } else {
                SyncStatus::Watching
            };
            records.insert(
                0,
                AnimeSyncRecord {
                    raw_title: params.raw_title.to_string(),
                    canonical_title: params.canonical_title.to_string(),
                    episode: params.episode,
                    path: params.path,
                    duration_secs: params.duration_secs,
                    watched_secs: actual_watched,
                    position_secs: params.position_secs,
                    watch_pct,
                    mal_anime_id: None,
                    mal_current_ep: None,
                    mal_total_episodes: None,
                    status,
                    last_updated: now,
                    intervals: params.intervals.to_vec(),
                },
            );
        }

        Self::save_all(&records)
    }

    pub fn update_progress(
        raw_title: &str,
        canonical_title: &str,
        episode: u32,
        path: Option<String>,
        duration_secs: u64,
        watched_secs: u64,
    ) -> Result<()> {
        Self::update_progress_full(ProgressUpdateParams {
            raw_title,
            canonical_title,
            episode,
            path,
            duration_secs,
            watched_secs,
            position_secs: None,
            intervals: &[],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_status_badge() {
        let synced = SyncStatus::Synced;
        assert_eq!(synced.badge().0, "✔ SYNCED");

        let ahead = SyncStatus::AheadOnMal { mal_episode: 6 };
        assert_eq!(ahead.badge().0, "⏩ MAL AHEAD");
    }

    #[test]
    fn test_anime_record_serialization() {
        let record = AnimeSyncRecord {
            raw_title: "[SubsPlease] Sousou no Frieren - 04 (1080p).mkv".into(),
            canonical_title: "Sousou no Frieren".into(),
            episode: 4,
            path: None,
            duration_secs: 1440,
            watched_secs: 1200,
            watch_pct: 83.3,
            mal_anime_id: Some(52991),
            mal_current_ep: Some(6),
            mal_total_episodes: Some(28),
            position_secs: Some(1200),
            status: SyncStatus::AheadOnMal { mal_episode: 6 },
            last_updated: "2026-09-07 16:40:00".into(),
            intervals: vec![PlaybackInterval {
                start: 0,
                end: 1200,
            }],
        };

        let json = serde_json::to_string(&record).expect("Failed to serialize");
        let parsed: AnimeSyncRecord = serde_json::from_str(&json).expect("Failed to deserialize");
        assert_eq!(parsed.canonical_title, "Sousou no Frieren");
        assert_eq!(parsed.episode, 4);
        assert_eq!(parsed.position_secs, Some(1200));
        assert_eq!(parsed.intervals.len(), 1);
        assert_eq!(parsed.status, SyncStatus::AheadOnMal { mal_episode: 6 });
    }

    #[test]
    fn test_update_progress_monotonic_watched_secs() {
        let raw = "Test Anime - 01.mkv";
        let title = "Test Anime";
        let ep = 1;

        // First watch 15 seconds
        AnimeSyncHistory::update_progress(raw, title, ep, None, 1000, 15).expect("updated");
        let r1 = AnimeSyncHistory::find_record(raw, title, ep).expect("found");
        assert_eq!(r1.watched_secs, 15);

        // User seeks back to 1 second
        AnimeSyncHistory::update_progress_full(ProgressUpdateParams {
            raw_title: raw,
            canonical_title: title,
            episode: ep,
            path: None,
            duration_secs: 1000,
            watched_secs: 1,
            position_secs: Some(1),
            intervals: &[],
        })
        .expect("updated");
        let r2 = AnimeSyncHistory::find_record(raw, title, ep).expect("found");
        // Watched duration MUST NOT DECREASE to 1; it must stay at 15
        assert_eq!(r2.watched_secs, 15);
        assert_eq!(r2.position_secs, Some(1));
    }
}
