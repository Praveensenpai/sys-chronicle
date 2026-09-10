use anyhow::{Context, Result};
use chrono::Local;
use std::env;
use std::io::{self, Read};
use std::process::Command;

use super::auth::MalAuth;
use super::client::MalClient;
use super::gemini::GeminiParser;
use super::history::{AnimeSyncHistory, AnimeSyncRecord, SyncStatus};
use super::parser::AnimeParser;
use crate::logger::ActivityEvent;

pub struct MalHandler;

impl MalHandler {
    pub async fn handle_stdin_hook() -> Result<()> {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;

        if input.trim().is_empty() {
            return Ok(());
        }

        let event: ActivityEvent = match serde_json::from_str(&input) {
            Ok(ev) => ev,
            Err(e) => {
                eprintln!("[sys-chronicle-mal] Ignoring non-ActivityEvent JSON: {}", e);
                return Ok(());
            }
        };

        match event {
            ActivityEvent::MediaWatchThreshold {
                title,
                path,
                duration_secs,
                watched_secs,
                watch_pct,
                ..
            } => {
                println!(
                    "[sys-chronicle-mal] Received 80% watch threshold for: \"{}\" ({:.1}%)",
                    title, watch_pct
                );
                let raw_media = path.as_deref().unwrap_or(&title);
                Self::sync_media_with_options(raw_media, duration_secs, watched_secs, false)
                    .await?;
            }
            ActivityEvent::MediaPlayback {
                title,
                path,
                position_secs,
                duration_secs,
                ..
            } => {
                let duration = duration_secs.unwrap_or(0);
                if duration > 0 {
                    let raw_media = path.as_deref().unwrap_or(&title).to_string();
                    let (canonical, ep) =
                        if let Some(super::classifier::MediaClassification::NonSyncable {
                            title: non_title,
                            ..
                        }) =
                            super::classifier::AnimeClassifier::check_local_heuristics(&raw_media)
                        {
                            (non_title, 0)
                        } else if let Some(info) = AnimeParser::parse(&raw_media) {
                            (info.title, info.episode)
                        } else {
                            (title.clone(), 0)
                        };
                    let _ = AnimeSyncHistory::update_progress_full(
                        crate::mal::history::ProgressUpdateParams {
                            raw_title: &raw_media,
                            canonical_title: &canonical,
                            episode: ep,
                            path,
                            duration_secs: duration,
                            watched_secs: 0,
                            position_secs: Some(position_secs),
                            intervals: &[],
                        },
                    );
                }
            }
            _ => {}
        }

        Ok(())
    }

    pub async fn sync_media(raw_media: &str) -> Result<()> {
        Self::sync_media_with_options(raw_media, 0, 0, false).await?;
        Ok(())
    }

    pub async fn sync_media_with_options(
        raw_media: &str,
        duration_secs: u64,
        watched_secs: u64,
        force: bool,
    ) -> Result<Option<AnimeSyncRecord>> {
        let mut config = MalAuth::load_config()?;
        let classification = Self::resolve_media_classification(raw_media, &config).await;

        let info = match classification {
            Some(super::classifier::MediaClassification::AnimeEpisode(info)) => info,
            Some(super::classifier::MediaClassification::NonSyncable {
                title,
                content_type,
                reason,
            }) => {
                println!(
                    "[sys-chronicle-mal] Skipping MAL sync for non-syncable media: \"{}\" ({}) - {}",
                    title, content_type, reason
                );
                let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                let existing = AnimeSyncHistory::find_record(raw_media, &title, 0);
                let final_dur = if duration_secs > 0 {
                    duration_secs
                } else {
                    existing.as_ref().map(|e| e.duration_secs).unwrap_or(0)
                };
                let final_watched = if watched_secs > 0 {
                    watched_secs
                } else {
                    existing.as_ref().map(|e| e.watched_secs).unwrap_or(0)
                };
                let record = AnimeSyncRecord {
                    raw_title: raw_media.to_string(),
                    canonical_title: title,
                    episode: 0,
                    path: Some(raw_media.to_string()),
                    duration_secs: final_dur,
                    watched_secs: final_watched,
                    position_secs: Some(final_watched),
                    watch_pct: if final_dur > 0 {
                        (final_watched as f32 / final_dur as f32 * 100.0).min(100.0)
                    } else {
                        0.0
                    },
                    mal_anime_id: None,
                    mal_current_ep: None,
                    mal_total_episodes: None,
                    status: SyncStatus::NonSyncable {
                        reason: content_type,
                    },
                    last_updated: now,
                    intervals: existing.map(|e| e.intervals).unwrap_or_default(),
                };
                let _ = AnimeSyncHistory::upsert(record.clone());
                return Ok(Some(record));
            }
            None => {
                println!(
                    "[sys-chronicle-mal] Could not identify anime title/episode from: \"{}\"",
                    raw_media
                );
                return Ok(None);
            }
        };

        println!(
            "[sys-chronicle-mal] Parsed: \"{}\" Episode {}",
            info.title, info.episode
        );

        let token = MalAuth::get_valid_token(&mut config).await?;
        let client = MalClient::new(token)?;

        let search_results = client.search_anime(&info.title).await?;
        let Some(anime) =
            super::candidate::select_best_candidate(&search_results, &info.title, info.season)
        else {
            println!(
                "[sys-chronicle-mal] No search results found on MAL for: \"{}\"",
                info.title
            );
            return Ok(None);
        };

        let current_watched = anime
            .my_list_status
            .as_ref()
            .map(|s| s.num_episodes_watched)
            .unwrap_or(0);

        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let is_completed = duration_secs > 0
            && (watched_secs >= duration_secs.saturating_sub(3)
                || (watched_secs as f64 / duration_secs as f64) >= 0.99);
        let (effective_watched, watch_pct) = if is_completed {
            (duration_secs, 100.0)
        } else if duration_secs > 0 {
            (
                watched_secs,
                (watched_secs as f32 / duration_secs as f32 * 100.0).min(100.0),
            )
        } else {
            (watched_secs, 100.0)
        };

        let status = if current_watched > info.episode && !force {
            println!(
                "[sys-chronicle-mal] MAL is already ahead at Episode {} for \"{}\" (watched: {}). Preserving higher progress.",
                current_watched, anime.title, info.episode
            );
            Self::send_notification(
                &anime.title,
                &format!(
                    "Ep {} watched (MAL is already at Ep {})",
                    info.episode, current_watched
                ),
            );
            SyncStatus::AheadOnMal {
                mal_episode: current_watched,
            }
        } else {
            if current_watched < info.episode || force {
                client
                    .update_episode_progress(anime.id, info.episode, anime.num_episodes)
                    .await
                    .with_context(|| {
                        format!("Failed to update MAL progress for anime ID {}", anime.id)
                    })?;
                println!(
                    "✔ Updated \"{}\" on MyAnimeList to Episode {} (total: {})",
                    anime.title, info.episode, anime.num_episodes
                );
            } else {
                println!(
                    "[sys-chronicle-mal] \"{}\" Episode {} already matches MAL progress.",
                    anime.title, info.episode
                );
            }
            Self::send_notification(
                &anime.title,
                &format!("Episode {} recorded on MyAnimeList", info.episode),
            );
            SyncStatus::Synced
        };

        let existing = AnimeSyncHistory::find_record(raw_media, &anime.title, info.episode);
        let intervals = existing
            .as_ref()
            .map(|e| e.intervals.clone())
            .unwrap_or_default();
        let position_secs = existing.as_ref().and_then(|e| e.position_secs);
        let existing_duration = existing.as_ref().map(|e| e.duration_secs).unwrap_or(0);
        let existing_watched = existing.as_ref().map(|e| e.watched_secs).unwrap_or(0);

        let final_duration = if duration_secs > 0 {
            duration_secs
        } else {
            existing_duration
        };
        let final_watched = if effective_watched > 0 {
            effective_watched
        } else {
            existing_watched
        };
        let final_pct = if final_duration > 0 && final_watched >= final_duration.saturating_sub(3) {
            100.0
        } else if final_duration > 0 {
            (final_watched as f32 / final_duration as f32 * 100.0).min(100.0)
        } else {
            watch_pct
        };

        let record = AnimeSyncRecord {
            raw_title: raw_media.to_string(),
            canonical_title: anime.title.clone(),
            episode: info.episode,
            path: Some(raw_media.to_string()),
            duration_secs: final_duration,
            watched_secs: final_watched,
            position_secs,
            watch_pct: final_pct,
            mal_anime_id: Some(anime.id),
            mal_current_ep: Some(if force {
                info.episode
            } else {
                current_watched.max(info.episode)
            }),
            mal_total_episodes: if anime.num_episodes > 0 {
                Some(anime.num_episodes)
            } else {
                None
            },
            status,
            last_updated: now,
            intervals,
        };

        let _ = AnimeSyncHistory::upsert(record.clone());
        Ok(Some(record))
    }

    async fn resolve_media_classification(
        raw_media: &str,
        config: &super::auth::MalConfig,
    ) -> Option<super::classifier::MediaClassification> {
        let gemini_key = env::var("GEMINI_API_KEY")
            .ok()
            .or_else(|| config.gemini_api_key.clone())
            .filter(|k| !k.trim().is_empty());

        let gemini =
            gemini_key.and_then(|k| GeminiParser::new(k, config.gemini_model.clone()).ok());

        super::classifier::AnimeClassifier::classify(raw_media, gemini.as_ref())
            .await
            .unwrap_or(None)
    }

    fn send_notification(title: &str, msg: &str) {
        let timeout_secs = MalAuth::load_config()
            .ok()
            .and_then(|c| c.toast_timeout_secs)
            .unwrap_or(2);
        let timeout_ms = (timeout_secs * 1000).to_string();

        let _ = Command::new("notify-send")
            .arg("-a")
            .arg("sys-chronicle-mal")
            .arg("-i")
            .arg("video-player")
            .arg("-t")
            .arg(&timeout_ms)
            .arg(format!("MAL: {}", title))
            .arg(msg)
            .status();
    }
}
