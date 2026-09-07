use anyhow::{Context, Result};
use std::env;
use std::io::{self, Read};
use std::process::Command;

use super::auth::MalAuth;
use super::client::MalClient;
use super::gemini::GeminiParser;
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

        if let ActivityEvent::MediaWatchThreshold {
            title,
            path,
            watch_pct,
            ..
        } = event
        {
            println!(
                "[sys-chronicle-mal] Received 80% watch threshold for: \"{}\" ({:.1}%)",
                title, watch_pct
            );
            let raw_media = path.as_deref().unwrap_or(&title);
            Self::sync_media(raw_media).await?;
        }

        Ok(())
    }

    pub async fn sync_media(raw_media: &str) -> Result<()> {
        let mut config = MalAuth::load_config()?;
        let anime_info = Self::resolve_anime_info(raw_media, &config).await;

        let Some(info) = anime_info else {
            println!(
                "[sys-chronicle-mal] Could not identify anime title/episode from: \"{}\"",
                raw_media
            );
            return Ok(());
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
            return Ok(());
        };

        let current_watched = anime
            .my_list_status
            .as_ref()
            .map(|s| s.num_episodes_watched)
            .unwrap_or(0);

        if current_watched >= info.episode {
            println!(
                "[sys-chronicle-mal] \"{}\" Episode {} already logged on MAL (current: {})",
                anime.title, info.episode, current_watched
            );
            return Ok(());
        }

        client
            .update_episode_progress(anime.id, info.episode, anime.num_episodes)
            .await
            .with_context(|| format!("Failed to update MAL progress for anime ID {}", anime.id))?;

        println!(
            "✔ Updated \"{}\" on MyAnimeList to Episode {} (total: {})",
            anime.title, info.episode, anime.num_episodes
        );

        Self::send_notification(&anime.title, info.episode);
        Ok(())
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
                Ok(gemini) => {
                    match gemini.parse(raw_media).await {
                        Ok(Some(info)) => return Some(info),
                        Ok(None) => {
                            eprintln!("[sys-chronicle-mal] Gemini returned empty match, trying regex parser...");
                        }
                        Err(e) => {
                            eprintln!("[sys-chronicle-mal] Gemini parsing error ({}); falling back to regex.", e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[sys-chronicle-mal] Failed to initialize Gemini parser ({}); falling back to regex.", e);
                }
            }
        } else {
            println!("[sys-chronicle-mal] Notice: Gemini API key not configured; using local regex parser.");
        }

        AnimeParser::parse(raw_media)
    }

    fn send_notification(title: &str, episode: u32) {
        let msg = format!("Episode {} recorded (80% watched)", episode);
        let _ = Command::new("notify-send")
            .arg("-a")
            .arg("sys-chronicle-mal")
            .arg("-i")
            .arg("video-player")
            .arg(format!("Synced to MAL: {}", title))
            .arg(msg)
            .status();
    }
}
