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
    NonSyncable { reason: String },
}

impl SyncStatus {
    pub fn badge(&self) -> (&'static str, ratatui::style::Color) {
        use ratatui::style::Color::*;
        match self {
            SyncStatus::Synced => ("✔ SYNCED", Green),
            SyncStatus::AheadOnMal { .. } => ("⏩ MAL AHEAD", Cyan),
            SyncStatus::ThresholdMet => ("⏳ PENDING", Yellow),
            SyncStatus::Watching => ("▶ WATCHING", Blue),
            SyncStatus::Failed { .. } => ("✖ FAILED", Red),
            SyncStatus::NonSyncable { reason } => match reason.as_str() {
                "live_action" => ("🎬 LIVE-ACT", Magenta),
                "special" => ("⭐ SPECIAL", Magenta),
                "bonus" => ("🎁 BONUS", DarkGray),
                "creditless" => ("🎵 NC OP/ED", DarkGray),
                _ => ("🚫 NOT ANIME", DarkGray),
            },
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

fn normalize_for_match(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn matches_record(
    r: &AnimeSyncRecord,
    raw_title: &str,
    canonical_title: &str,
    episode: u32,
) -> bool {
    if r.raw_title == raw_title || r.path.as_deref() == Some(raw_title) {
        return true;
    }
    let file_b = std::path::Path::new(raw_title).file_name();
    let file_matches = file_b.is_some()
        && (std::path::Path::new(&r.raw_title).file_name() == file_b
            || r.path
                .as_deref()
                .map(std::path::Path::new)
                .and_then(|p| p.file_name())
                == file_b);
    if file_matches {
        return true;
    }
    if (episode > 0 || r.episode > 0) && r.episode != episode {
        return false;
    }
    let n1 = normalize_for_match(&r.canonical_title);
    let n2 = normalize_for_match(canonical_title);
    !n1.is_empty() && n1 == n2 && r.episode == episode
}

fn compute_watch_pct(watched: u64, duration: u64, is_completed: bool) -> f32 {
    if is_completed {
        100.0
    } else if duration > 0 {
        (watched as f32 / duration as f32 * 100.0).min(100.0)
    } else {
        0.0
    }
}

fn apply_record_progress(
    r: &mut AnimeSyncRecord,
    params: &ProgressUpdateParams<'_>,
    effective_watched: u64,
    non_sync_reason: Option<String>,
) {
    if !params.intervals.is_empty() {
        r.intervals = crate::monitor::timeline_tracker::UniqueTimelineTracker::merge_interval_lists(
            &r.intervals,
            params.intervals,
        );
    }
    let unique_secs = r
        .intervals
        .iter()
        .map(|i| i.end.saturating_sub(i.start))
        .sum::<u64>();
    let watched = unique_secs.max(r.watched_secs).max(effective_watched);
    r.watched_secs = watched;
    r.duration_secs = params.duration_secs;
    r.position_secs = params.position_secs.or(r.position_secs);

    let is_comp = params.duration_secs > 0
        && (watched >= params.duration_secs.saturating_sub(3)
            || (watched as f64 / params.duration_secs as f64) >= 0.99);

    r.watch_pct = compute_watch_pct(watched, params.duration_secs, is_comp);
    r.last_updated = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    if let Some(reason) = non_sync_reason {
        r.status = SyncStatus::NonSyncable { reason };
        r.episode = 0;
    } else if r.status == SyncStatus::Watching && r.watch_pct >= 80.0 {
        r.status = SyncStatus::ThresholdMet;
    }
}

impl AnimeSyncHistory {
    pub fn storage_path() -> PathBuf {
        #[cfg(test)]
        return std::env::temp_dir().join("sys_chronicle_test_anime_sync_history.json");
        #[cfg(not(test))]
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("sys-chronicle")
            .join("anime_sync_history.json")
    }

    pub fn deduplicate(records: Vec<AnimeSyncRecord>) -> Vec<AnimeSyncRecord> {
        let mut deduped: Vec<AnimeSyncRecord> = Vec::with_capacity(records.len());
        for rec in records {
            if let Some(existing) = deduped
                .iter_mut()
                .find(|r| matches_record(r, &rec.raw_title, &rec.canonical_title, rec.episode))
            {
                if rec.watch_pct > existing.watch_pct || rec.watched_secs > existing.watched_secs {
                    existing.watch_pct = existing.watch_pct.max(rec.watch_pct);
                    existing.watched_secs = existing.watched_secs.max(rec.watched_secs);
                    existing.position_secs = rec.position_secs.or(existing.position_secs);
                    existing.status = rec.status;
                }
                if !rec.intervals.is_empty() {
                    existing.intervals =
                        crate::monitor::timeline_tracker::UniqueTimelineTracker::merge_interval_lists(
                            &existing.intervals,
                            &rec.intervals,
                        );
                }
                if rec.last_updated > existing.last_updated {
                    existing.last_updated = rec.last_updated;
                }
            } else {
                deduped.push(rec);
            }
        }
        deduped
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
        let records: Vec<AnimeSyncRecord> = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse JSON history from {:?}", path))?;
        Ok(Self::deduplicate(records))
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
            matches_record(
                r,
                &record.raw_title,
                &record.canonical_title,
                record.episode,
            )
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
        Self::load_all()
            .ok()?
            .into_iter()
            .find(|r| matches_record(r, raw_title, canonical_title, episode))
    }

    pub fn update_progress_full(params: ProgressUpdateParams<'_>) -> Result<()> {
        let mut records = Self::load_all().unwrap_or_default();
        let is_completed = params.duration_secs > 0
            && (params.watched_secs >= params.duration_secs.saturating_sub(3)
                || (params.watched_secs as f64 / params.duration_secs as f64) >= 0.99);
        let effective_watched = if is_completed {
            params.duration_secs
        } else {
            params.watched_secs
        };

        let media_path = params.path.as_deref().unwrap_or(params.raw_title);
        let classification = super::classifier::AnimeClassifier::check_local_heuristics(media_path);
        let (canonical, episode, non_sync_reason) = match classification {
            Some(super::classifier::MediaClassification::NonSyncable { title, reason, .. }) => {
                (title, 0, Some(reason))
            }
            _ => (params.canonical_title.to_string(), params.episode, None),
        };

        if let Some(r) = records
            .iter_mut()
            .find(|r| matches_record(r, params.raw_title, &canonical, episode))
        {
            apply_record_progress(r, &params, effective_watched, non_sync_reason);
        } else {
            let watch_pct =
                compute_watch_pct(effective_watched, params.duration_secs, is_completed);
            let status = match non_sync_reason {
                Some(reason) => SyncStatus::NonSyncable { reason },
                None if watch_pct >= 80.0 => SyncStatus::ThresholdMet,
                None => SyncStatus::Watching,
            };
            records.insert(
                0,
                AnimeSyncRecord {
                    raw_title: params.raw_title.to_string(),
                    canonical_title: canonical,
                    episode,
                    path: params.path,
                    duration_secs: params.duration_secs,
                    watched_secs: effective_watched,
                    position_secs: params.position_secs,
                    watch_pct,
                    mal_anime_id: None,
                    mal_current_ep: None,
                    mal_total_episodes: None,
                    status,
                    last_updated: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
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
mod tests;
