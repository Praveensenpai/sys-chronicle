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
    #[serde(default)]
    pub mal_title: Option<String>,
    #[serde(default)]
    pub mal_title_en: Option<String>,
    #[serde(default)]
    pub mal_title_ja: Option<String>,
    #[serde(default)]
    pub mal_image_url: Option<String>,
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

/// Per-anime reconciled metadata.
#[derive(Default, Clone)]
struct AnimeMeta {
    mal_id: Option<u64>,
    max_synced_ep: Option<u32>,
    total_eps: Option<u32>,
    mal_title: Option<String>,
    mal_title_en: Option<String>,
    mal_title_ja: Option<String>,
    mal_image_url: Option<String>,
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

    let p_a = r.path.as_deref().unwrap_or(&r.raw_title);
    let p_b = raw_title;

    if p_a.contains('/') && p_b.contains('/') {
        let parent_a = std::path::Path::new(p_a).parent();
        let parent_b = std::path::Path::new(p_b).parent();
        if let (Some(pa), Some(pb)) = (parent_a, parent_b) {
            if !pa.as_os_str().is_empty() && !pb.as_os_str().is_empty() && pa != pb {
                return false;
            }
        }
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
    // Distinct files must not match unless both have a positive episode number
    // that matches and matching canonical title. Episode 0 is reserved for
    // non-syncable or unparsed entries where each file is independent.
    if episode == 0 || r.episode == 0 {
        return false;
    }
    let n1 = normalize_for_match(&r.canonical_title);
    let n2 = normalize_for_match(canonical_title);
    !n1.is_empty() && n1 == n2
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
    if params.duration_secs > 0 {
        r.duration_secs = params.duration_secs;
    }
    r.position_secs = params.position_secs.or(r.position_secs);

    let is_comp = r.duration_secs > 0
        && (watched >= r.duration_secs.saturating_sub(3)
            || (watched as f64 / r.duration_secs as f64) >= 0.99);

    let new_pct = compute_watch_pct(watched, r.duration_secs, is_comp);
    r.watch_pct = r.watch_pct.max(new_pct);
    r.last_updated = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    if let Some(reason) = non_sync_reason {
        r.status = SyncStatus::NonSyncable { reason };
        r.episode = 0;
    } else {
        let new_status = if r.watch_pct >= 80.0 {
            SyncStatus::ThresholdMet
        } else {
            SyncStatus::Watching
        };
        r.status = higher_status(r.status.clone(), new_status);
    }
}

fn higher_status(a: SyncStatus, b: SyncStatus) -> SyncStatus {
    fn rank(s: &SyncStatus) -> u8 {
        match s {
            SyncStatus::Synced => 4,
            SyncStatus::AheadOnMal { .. } => 3,
            SyncStatus::ThresholdMet => 2,
            SyncStatus::Watching => 1,
            SyncStatus::Failed { .. } | SyncStatus::NonSyncable { .. } => 0,
        }
    }
    if rank(&b) > rank(&a) {
        b
    } else {
        a
    }
}

fn fill_missing_episode_numbers(records: &mut [AnimeSyncRecord]) {
    for r in records {
        if r.episode == 0 {
            let target = r.path.as_deref().unwrap_or(&r.raw_title);
            if let Some(info) = super::parser::AnimeParser::parse(target) {
                r.episode = info.episode;
            } else if r.mal_total_episodes == Some(1) {
                r.episode = 1;
            }
        }
    }
}

fn collapse_duplicate_records(records: Vec<AnimeSyncRecord>) -> Vec<AnimeSyncRecord> {
    let mut deduped: Vec<AnimeSyncRecord> = Vec::with_capacity(records.len());
    for rec in records {
        if let Some(existing) = deduped
            .iter_mut()
            .find(|r| matches_record(r, &rec.raw_title, &rec.canonical_title, rec.episode))
        {
            merge_record_fields(existing, &rec);
        } else {
            deduped.push(rec);
        }
    }
    deduped
}

fn merge_record_fields(existing: &mut AnimeSyncRecord, rec: &AnimeSyncRecord) {
    existing.watch_pct = existing.watch_pct.max(rec.watch_pct);
    existing.watched_secs = existing.watched_secs.max(rec.watched_secs);
    existing.position_secs = rec.position_secs.or(existing.position_secs);
    existing.status = higher_status(existing.status.clone(), rec.status.clone());
    existing.mal_anime_id = existing.mal_anime_id.or(rec.mal_anime_id);
    existing.mal_current_ep = existing.mal_current_ep.or(rec.mal_current_ep);
    existing.mal_total_episodes = existing.mal_total_episodes.or(rec.mal_total_episodes);
    existing.mal_title = existing.mal_title.take().or_else(|| rec.mal_title.clone());
    existing.mal_title_en = existing.mal_title_en.take().or_else(|| rec.mal_title_en.clone());
    existing.mal_title_ja = existing.mal_title_ja.take().or_else(|| rec.mal_title_ja.clone());
    existing.mal_image_url = existing.mal_image_url.take().or_else(|| rec.mal_image_url.clone());

    if !rec.intervals.is_empty() {
        existing.intervals =
            crate::monitor::timeline_tracker::UniqueTimelineTracker::merge_interval_lists(
                &existing.intervals,
                &rec.intervals,
            );
    }
    if rec.last_updated > existing.last_updated {
        existing.last_updated = rec.last_updated.clone();
    }
}

fn reconcile_records_metadata(records: &mut [AnimeSyncRecord]) {
    let mut by_id: std::collections::HashMap<u64, AnimeMeta> =
        std::collections::HashMap::new();
    let mut by_title: std::collections::HashMap<String, AnimeMeta> =
        std::collections::HashMap::new();

    for r in records.iter() {
        if let Some(id) = r.mal_anime_id {
            let entry = by_id.entry(id).or_default();
            entry.mal_id = Some(id);
            if let Some(cur) = r.mal_current_ep {
                entry.max_synced_ep = Some(entry.max_synced_ep.unwrap_or(0).max(cur));
            }
            if r.status == SyncStatus::Synced {
                entry.max_synced_ep = Some(entry.max_synced_ep.unwrap_or(0).max(r.episode));
            }
            if r.mal_total_episodes.is_some() && entry.total_eps.is_none() {
                entry.total_eps = r.mal_total_episodes;
            }
            if r.mal_title.is_some() && entry.mal_title.is_none() {
                entry.mal_title = r.mal_title.clone();
            }
            if r.mal_title_en.is_some() && entry.mal_title_en.is_none() {
                entry.mal_title_en = r.mal_title_en.clone();
            }
            if r.mal_title_ja.is_some() && entry.mal_title_ja.is_none() {
                entry.mal_title_ja = r.mal_title_ja.clone();
            }
            if r.mal_image_url.is_some() && entry.mal_image_url.is_none() {
                entry.mal_image_url = r.mal_image_url.clone();
            }

            let is_special = entry.total_eps == Some(1)
                || r.canonical_title.to_lowercase().contains("ova")
                || r.raw_title.to_lowercase().contains("ova");
            if !is_special {
                let key = r.canonical_title.trim().to_lowercase();
                let t_entry = by_title.entry(key).or_default();
                if t_entry.mal_id.is_none() {
                    t_entry.mal_id = Some(id);
                    t_entry.total_eps = entry.total_eps;
                    t_entry.mal_title = entry.mal_title.clone();
                    t_entry.mal_title_en = entry.mal_title_en.clone();
                    t_entry.mal_title_ja = entry.mal_title_ja.clone();
                    t_entry.mal_image_url = entry.mal_image_url.clone();
                }
                if let Some(max_ep) = entry.max_synced_ep {
                    t_entry.max_synced_ep =
                        Some(t_entry.max_synced_ep.unwrap_or(0).max(max_ep));
                }
            }
        }
    }

    for r in records.iter_mut() {
        let meta_opt = if let Some(id) = r.mal_anime_id {
            by_id.get(&id)
        } else {
            let key = r.canonical_title.trim().to_lowercase();
            by_title.get(&key)
        };

        if let Some(meta) = meta_opt {
            apply_meta_to_record(r, meta);
        }
    }
}

fn apply_meta_to_record(r: &mut AnimeSyncRecord, meta: &AnimeMeta) {
    r.mal_anime_id = r.mal_anime_id.or(meta.mal_id);
    r.mal_total_episodes = r.mal_total_episodes.or(meta.total_eps);
    r.mal_current_ep = r.mal_current_ep.or(meta.max_synced_ep);
    if r.mal_title.is_none() {
        r.mal_title = meta.mal_title.clone();
    }
    if r.mal_title_en.is_none() {
        r.mal_title_en = meta.mal_title_en.clone();
    }
    if r.mal_title_ja.is_none() {
        r.mal_title_ja = meta.mal_title_ja.clone();
    }
    if r.mal_image_url.is_none() {
        r.mal_image_url = meta.mal_image_url.clone();
    }

    if let Some(max_ep) = meta.max_synced_ep {
        if r.episode > 0 && r.episode <= max_ep && r.status == SyncStatus::Watching {
            r.status = SyncStatus::Synced;
            if r.duration_secs > 0 {
                r.watched_secs = r.watched_secs.max(r.duration_secs);
                r.watch_pct = r.watch_pct.max(100.0);
                if r.intervals.is_empty() {
                    r.intervals = vec![crate::monitor::timeline_tracker::PlaybackInterval {
                        start: 0,
                        end: r.duration_secs,
                    }];
                }
            }
        }
    }
}

fn sort_deduped_records(records: &mut [AnimeSyncRecord]) {
    let mut anime_latest: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for r in records.iter() {
        let key = r.canonical_title.trim().to_lowercase();
        let latest = anime_latest.entry(key).or_default();
        if r.last_updated > *latest {
            *latest = r.last_updated.clone();
        }
    }

    records.sort_by(|a, b| {
        let key_a = a.canonical_title.trim().to_lowercase();
        let key_b = b.canonical_title.trim().to_lowercase();
        let time_a = anime_latest.get(&key_a);
        let time_b = anime_latest.get(&key_b);

        time_b
            .cmp(&time_a)
            .then_with(|| a.canonical_title.cmp(&b.canonical_title))
            .then_with(|| a.episode.cmp(&b.episode))
    });
}

impl AnimeSyncHistory {
    pub fn storage_path() -> PathBuf {
        #[cfg(test)]
        {
            // History tests run in parallel threads and each one expects an
            // isolated store, so scope the temp file to the current thread.
            use std::sync::OnceLock;
            thread_local! {
                static TEST_PATH: OnceLock<PathBuf> = const { OnceLock::new() };
            }
            TEST_PATH.with(|slot| {
                slot.get_or_init(|| {
                    std::env::temp_dir().join(format!(
                        "sys_chronicle_test_anime_sync_history_{:?}.json",
                        std::thread::current().id()
                    ))
                })
                .clone()
            })
        }
        #[cfg(not(test))]
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("sys-chronicle")
            .join("anime_sync_history.json")
    }

    pub fn deduplicate(mut records: Vec<AnimeSyncRecord>) -> Vec<AnimeSyncRecord> {
        fill_missing_episode_numbers(&mut records);
        let mut deduped = collapse_duplicate_records(records);
        reconcile_records_metadata(&mut deduped);
        sort_deduped_records(&mut deduped);
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
        let tmp_path = path.with_extension(format!("tmp.{}", std::process::id()));
        {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp_path)?;
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp_path, &path)?;
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
            let existing = &mut records[idx];
            if !record.intervals.is_empty() {
                existing.intervals =
                    crate::monitor::timeline_tracker::UniqueTimelineTracker::merge_interval_lists(
                        &existing.intervals,
                        &record.intervals,
                    );
            }
            let unique_secs: u64 = existing
                .intervals
                .iter()
                .map(|i| i.end.saturating_sub(i.start))
                .sum();
            existing.watched_secs = unique_secs
                .max(existing.watched_secs)
                .max(record.watched_secs);
            existing.duration_secs = record.duration_secs;
            existing.position_secs = record.position_secs.or(existing.position_secs);
            existing.watch_pct = existing.watch_pct.max(record.watch_pct);
            existing.last_updated = record.last_updated;
            existing.mal_anime_id = record.mal_anime_id.or(existing.mal_anime_id);
            existing.mal_current_ep = record.mal_current_ep.or(existing.mal_current_ep);
            existing.mal_total_episodes = record.mal_total_episodes.or(existing.mal_total_episodes);
            existing.mal_title = record.mal_title.or(existing.mal_title.clone());
            existing.mal_title_en = record.mal_title_en.or(existing.mal_title_en.clone());
            existing.mal_title_ja = record.mal_title_ja.or(existing.mal_title_ja.clone());
            existing.mal_image_url = record.mal_image_url.or(existing.mal_image_url.clone());
            existing.status = higher_status(existing.status.clone(), record.status);
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
        let override_info = super::overrides::AnimeOverrides::load()
            .ok()
            .and_then(|o| o.lookup(media_path).cloned());

        let classification = super::classifier::AnimeClassifier::check_local_heuristics(media_path);
        let (canonical, episode, non_sync_reason) = match classification {
            Some(super::classifier::MediaClassification::NonSyncable { title, reason, .. }) => {
                (title, 0, Some(reason))
            }
            _ => {
                if let Some(ref ov) = override_info {
                    (ov.canonical_title.clone(), params.episode, None)
                } else {
                    (params.canonical_title.to_string(), params.episode, None)
                }
            }
        };

        let (init_mal_id, init_total) = match override_info {
            Some(ref ov) if non_sync_reason.is_none() => (Some(ov.mal_anime_id), ov.total_episodes),
            _ => (None, None),
        };

        if let Some(r) = records
            .iter_mut()
            .find(|r| matches_record(r, params.raw_title, &canonical, episode))
        {
            if let Some(mal_id) = init_mal_id {
                if r.mal_anime_id.is_none() {
                    r.mal_anime_id = Some(mal_id);
                    r.canonical_title = canonical;
                    r.mal_total_episodes = init_total;
                }
            }
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
                    mal_anime_id: init_mal_id,
                    mal_current_ep: None,
                    mal_total_episodes: init_total,
                    mal_title: None,
                    mal_title_en: None,
                    mal_title_ja: None,
                    mal_image_url: None,
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
