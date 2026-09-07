use anyhow::{Context, Result};
use chrono::Local;
use std::env;
use std::io::{self, Read};
use std::process::Command;

use super::auth::MalAuth;
use super::client::MalClient;
use super::gemini::GeminiParser;
use super::history::{AnimeSyncHistory, AnimeSyncRecord, SyncStatus};
use super::parser::{AnimeInfo, AnimeParser};
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
                    if let Some(info) = AnimeParser::parse(&raw_media) {
                        let _ = AnimeSyncHistory::update_progress(
                            &raw_media,
                            &info.title,
                            info.episode,
                            path,
                            duration,
                            position_secs,
                        );
                    }
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
        let anime_info = Self::resolve_anime_info(raw_media, &config).await;

        let Some(info) = anime_info else {
            println!(
                "[sys-chronicle-mal] Could not identify anime title/episode from: \"{}\"",
                raw_media
            );
            return Ok(None);
        };

        println!(
            "[sys-chronicle-mal] Parsed: \"{}\" Episode {}",
            info.title, info.episode
        );

        let token = MalAuth::get_valid_token(&mut config).await?;
        let client = MalClient::new(token)?;

        let search_results = client.search_anime(&info.title).await?;
        let Some(anime) = search_results.into_iter().next() else {
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
        let watch_pct = if duration_secs > 0 {
            (watched_secs as f32 / duration_secs as f32 * 100.0).min(100.0)
        } else {
            100.0
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

        let record = AnimeSyncRecord {
            raw_title: raw_media.to_string(),
            canonical_title: anime.title,
            episode: info.episode,
            path: Some(raw_media.to_string()),
            duration_secs,
            watched_secs,
            watch_pct,
            mal_anime_id: Some(anime.id),
            mal_current_ep: Some(current_watched.max(info.episode)),
            mal_total_episodes: if anime.num_episodes > 0 {
                Some(anime.num_episodes)
            } else {
                None
            },
            status,
            last_updated: now,
        };

        let _ = AnimeSyncHistory::upsert(record.clone());
        Ok(Some(record))
    }

    async fn resolve_anime_info(
        raw_media: &str,
        config: &super::auth::MalConfig,
    ) -> Option<AnimeInfo> {
        let gemini_key = env::var("GEMINI_API_KEY")
            .ok()
            .or_else(|| config.gemini_api_key.clone())
            .filter(|k| !k.trim().is_empty());

        if let Some(key) = gemini_key {
            match GeminiParser::new(key, config.gemini_model.clone()) {
                Ok(gemini) => match gemini.parse(raw_media).await {
                    Ok(Some(info)) => return Some(info),
                    Ok(None) => {
                        eprintln!("[sys-chronicle-mal] Gemini returned empty match, trying regex parser...");
                    }
                    Err(e) => {
                        eprintln!(
                            "[sys-chronicle-mal] Gemini parsing error ({}); falling back to regex.",
                            e
                        );
                    }
                },
                Err(e) => {
                    eprintln!(
                        "[sys-chronicle-mal] Failed to initialize Gemini parser ({}); falling back to regex.",
                        e
                    );
                }
            }
        } else {
            println!("[sys-chronicle-mal] Notice: Gemini API key not configured; using local regex parser.");
        }

        AnimeParser::parse(raw_media)
    }

    fn send_notification(title: &str, msg: &str) {
        let _ = Command::new("notify-send")
            .arg("-a")
            .arg("sys-chronicle-mal")
            .arg("-i")
            .arg("video-player")
            .arg(format!("MAL: {}", title))
            .arg(msg)
            .status();
    }
}
