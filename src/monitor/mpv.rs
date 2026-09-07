use anyhow::Result;
use chrono::Local;
use serde::Deserialize;
use std::env;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::time::{sleep, Duration};

use crate::logger::{ActivityEvent, LogWriter};

pub struct MpvMonitor {
    writer: LogWriter,
}

#[derive(Debug, Deserialize)]
struct MpvEvent {
    event: Option<String>,
    id: Option<u64>,
    data: Option<serde_json::Value>,
}

struct MpvProgressParams<'a> {
    title: &'a str,
    path: Option<&'a str>,
    duration_secs: Option<u64>,
    watched_secs: u64,
    position_secs: u64,
    intervals: &'a [crate::monitor::PlaybackInterval],
}

impl MpvMonitor {
    pub fn new(writer: LogWriter) -> Self {
        Self { writer }
    }

    fn candidate_sockets() -> Vec<PathBuf> {
        let mut paths = Vec::new();
        paths.push(PathBuf::from("/tmp/mpvsocket"));

        if let Ok(runtime) = env::var("XDG_RUNTIME_DIR") {
            paths.push(PathBuf::from(&runtime).join("mpvsocket"));
            paths.push(PathBuf::from(&runtime).join("mpv.sock"));
        } else if let Some(runtime) = dirs::runtime_dir() {
            paths.push(runtime.join("mpvsocket"));
        }

        if let Some(home) = dirs::home_dir() {
            paths.push(home.join(".config").join("mpv").join("socket"));
        }

        paths
    }

    fn find_active_socket() -> Option<PathBuf> {
        Self::candidate_sockets()
            .into_iter()
            .find(|path| path.exists())
    }

    pub async fn run(&mut self, running: Arc<AtomicBool>) -> Result<()> {
        while running.load(Ordering::SeqCst) {
            if let Some(socket_path) = Self::find_active_socket() {
                if let Ok(stream) = UnixStream::connect(&socket_path).await {
                    let _ = self.handle_mpv_session(stream, running.clone()).await;
                }
            }
            sleep(Duration::from_secs(2)).await;
        }
        Ok(())
    }

    async fn handle_mpv_session(
        &mut self,
        stream: UnixStream,
        running: Arc<AtomicBool>,
    ) -> Result<()> {
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();

        // Subscribe to mpv properties
        let commands = [
            r#"{"command": ["observe_property", 1, "media-title"]}"#,
            r#"{"command": ["observe_property", 2, "path"]}"#,
            r#"{"command": ["observe_property", 3, "pause"]}"#,
            r#"{"command": ["observe_property", 4, "time-pos"]}"#,
            r#"{"command": ["observe_property", 5, "duration"]}"#,
        ];

        for cmd in &commands {
            writer.write_all(cmd.as_bytes()).await?;
            writer.write_all(b"\n").await?;
        }
        writer.flush().await?;

        let mut current_title = String::new();
        let mut current_path: Option<String> = None;
        let mut current_pause = false;
        let mut current_pos = 0u64;
        let mut current_duration: Option<u64> = None;
        let mut last_pos_update = Local::now();
        let mut last_history_sync = std::time::Instant::now();
        let mut session_started = false;
        let mut tracker = super::UniqueTimelineTracker::new();

        while running.load(Ordering::SeqCst) {
            tokio::select! {
                line = lines.next_line() => {
                    match line {
                        Ok(Some(raw_line)) => {
                            if let Ok(msg) = serde_json::from_str::<MpvEvent>(&raw_line) {
                                if msg.event.as_deref() == Some("property-change") {
                                    match msg.id {
                                        Some(1) => { // media-title
                                            if let Some(val) = msg.data.as_ref().and_then(|v| v.as_str()) {
                                                let new_title = val.trim().to_string();
                                                if !new_title.is_empty() && new_title != current_title {
                                                    current_title = new_title;
                                                    tracker.set_media(&current_title, current_duration);
                                                    let raw = current_path.as_deref().unwrap_or(&current_title);
                                                    if let Some(info) = crate::mal::AnimeParser::parse(raw) {
                                                        if let Some(existing) = crate::mal::AnimeSyncHistory::find_record(raw, &info.title, info.episode) {
                                                            if !existing.intervals.is_empty() {
                                                                tracker.load_intervals(&existing.intervals);
                                                            }
                                                        }
                                                    }
                                                    self.log_media_event("start", &current_title, current_path.as_deref(), current_pos, current_duration);
                                                    Self::sync_history_progress(MpvProgressParams {
                                                        title: &current_title,
                                                        path: current_path.as_deref(),
                                                        duration_secs: current_duration,
                                                        watched_secs: tracker.total_unique_secs(),
                                                        position_secs: current_pos,
                                                        intervals: tracker.intervals(),
                                                    });
                                                    session_started = true;
                                                }
                                            }
                                        }
                                        Some(2) => { // path
                                            if let Some(val) = msg.data.as_ref().and_then(|v| v.as_str()) {
                                                current_path = Some(val.to_string());
                                                if current_title.is_empty() {
                                                    let file_name = std::path::Path::new(val)
                                                        .file_name()
                                                        .map(|n| n.to_string_lossy().to_string())
                                                        .unwrap_or_else(|| val.to_string());
                                                    current_title = file_name;
                                                    tracker.set_media(&current_title, current_duration);
                                                    let raw = current_path.as_deref().unwrap_or(&current_title);
                                                    if let Some(info) = crate::mal::AnimeParser::parse(raw) {
                                                        if let Some(existing) = crate::mal::AnimeSyncHistory::find_record(raw, &info.title, info.episode) {
                                                            if !existing.intervals.is_empty() {
                                                                tracker.load_intervals(&existing.intervals);
                                                            }
                                                        }
                                                    }
                                                    self.log_media_event("start", &current_title, current_path.as_deref(), current_pos, current_duration);
                                                    Self::sync_history_progress(MpvProgressParams {
                                                        title: &current_title,
                                                        path: current_path.as_deref(),
                                                        duration_secs: current_duration,
                                                        watched_secs: tracker.total_unique_secs(),
                                                        position_secs: current_pos,
                                                        intervals: tracker.intervals(),
                                                    });
                                                    session_started = true;
                                                }
                                            }
                                        }
                                        Some(3) => { // pause
                                            if let Some(paused) = msg.data.as_ref().and_then(|v| v.as_bool()) {
                                                if paused != current_pause {
                                                    current_pause = paused;
                                                    let action = if paused { "pause" } else { "resume" };
                                                    self.log_media_event(action, &current_title, current_path.as_deref(), current_pos, current_duration);
                                                    Self::sync_history_progress(MpvProgressParams {
                                                        title: &current_title,
                                                        path: current_path.as_deref(),
                                                        duration_secs: current_duration,
                                                        watched_secs: tracker.total_unique_secs(),
                                                        position_secs: current_pos,
                                                        intervals: tracker.intervals(),
                                                    });
                                                }
                                            }
                                        }
                                        Some(4) => { // time-pos
                                            if let Some(pos_num) = msg.data.as_ref().and_then(|v| v.as_f64()) {
                                                let new_pos = pos_num.max(0.0) as u64;
                                                let now = Local::now();
                                                let elapsed_wall = (now - last_pos_update).num_seconds().max(0) as u64;

                                                // Detect seek if position jumped significantly (> 5s difference from expected time)
                                                if session_started && !current_pause {
                                                    let expected_pos = current_pos + elapsed_wall;
                                                    let diff = (new_pos as i64 - expected_pos as i64).abs();
                                                    if diff > 5 {
                                                        let seek_action = if new_pos > current_pos { "seek_forward" } else { "seek_backward" };
                                                        self.log_media_event(seek_action, &current_title, current_path.as_deref(), new_pos, current_duration);
                                                    }
                                                }

                                                tracker.record_position(new_pos, current_pause);
                                                if let Some((dur, watched, pct)) = tracker.check_threshold(80.0) {
                                                    self.log_threshold_event(
                                                        &current_title,
                                                        current_path.as_deref(),
                                                        dur,
                                                        watched,
                                                        pct,
                                                    );
                                                }

                                                if last_history_sync.elapsed() >= std::time::Duration::from_secs(3) {
                                                    Self::sync_history_progress(MpvProgressParams {
                                                        title: &current_title,
                                                        path: current_path.as_deref(),
                                                        duration_secs: current_duration,
                                                        watched_secs: tracker.total_unique_secs(),
                                                        position_secs: new_pos,
                                                        intervals: tracker.intervals(),
                                                    });
                                                    last_history_sync = std::time::Instant::now();
                                                }

                                                current_pos = new_pos;
                                                last_pos_update = now;
                                            }
                                        }
                                        Some(5) => { // duration
                                            if let Some(dur_num) = msg.data.as_ref().and_then(|v| v.as_f64()) {
                                                let dur = dur_num.max(0.0) as u64;
                                                current_duration = Some(dur);
                                                tracker.set_duration(dur);
                                                Self::sync_history_progress(MpvProgressParams {
                                                    title: &current_title,
                                                    path: current_path.as_deref(),
                                                    duration_secs: current_duration,
                                                    watched_secs: tracker.total_unique_secs(),
                                                    position_secs: current_pos,
                                                    intervals: tracker.intervals(),
                                                });
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        Ok(None) | Err(_) => break,
                    }
                }
                _ = sleep(Duration::from_secs(1)) => {}
            }
        }

        if session_started && !current_title.is_empty() {
            self.log_media_event(
                "stop",
                &current_title,
                current_path.as_deref(),
                current_pos,
                current_duration,
            );
            Self::sync_history_progress(MpvProgressParams {
                title: &current_title,
                path: current_path.as_deref(),
                duration_secs: current_duration,
                watched_secs: tracker.total_unique_secs(),
                position_secs: current_pos,
                intervals: tracker.intervals(),
            });
        }

        Ok(())
    }

    fn log_media_event(
        &self,
        event_type: &str,
        title: &str,
        path: Option<&str>,
        position_secs: u64,
        duration_secs: Option<u64>,
    ) {
        if title.is_empty() {
            return;
        }

        let now = Local::now();
        let timestamp_str = now.format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string();

        let event = ActivityEvent::MediaPlayback {
            timestamp: timestamp_str,
            player: "mpv".to_string(),
            event_type: event_type.to_string(),
            title: title.to_string(),
            path: path.map(|p| p.to_string()),
            position_secs,
            duration_secs,
        };

        let _ = self.writer.write_event(&event);
        crate::plugin::PluginDispatcher::dispatch_event(&event);
    }

    fn sync_history_progress(params: MpvProgressParams<'_>) {
        let raw = params.path.unwrap_or(params.title);
        if let Some(info) = crate::mal::AnimeParser::parse(raw) {
            let duration = params.duration_secs.unwrap_or(0);
            if duration > 0 {
                let _ = crate::mal::AnimeSyncHistory::update_progress_full(
                    crate::mal::history::ProgressUpdateParams {
                        raw_title: raw,
                        canonical_title: &info.title,
                        episode: info.episode,
                        path: params.path.map(|s| s.to_string()),
                        duration_secs: duration,
                        watched_secs: params.watched_secs,
                        position_secs: Some(params.position_secs),
                        intervals: params.intervals,
                    },
                );
            }
        }
    }

    fn log_threshold_event(
        &self,
        title: &str,
        path: Option<&str>,
        duration_secs: u64,
        watched_secs: u64,
        watch_pct: f32,
    ) {
        if title.is_empty() {
            return;
        }

        let now = Local::now();
        let timestamp_str = now.format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string();

        let event = ActivityEvent::MediaWatchThreshold {
            timestamp: timestamp_str,
            player: "mpv".to_string(),
            title: title.to_string(),
            path: path.map(|p| p.to_string()),
            duration_secs,
            watched_secs,
            watch_pct,
        };

        let _ = self.writer.write_event(&event);
        crate::plugin::PluginDispatcher::dispatch_event(&event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_history_progress_creates_record() {
        let title = "Saijo no Osewa - 10.mkv";
        let path = Some("/home/paisen/Videos/Anime/Saijo no Osewa - 10.mkv");
        let intervals = vec![crate::monitor::PlaybackInterval { start: 0, end: 6 }];
        MpvMonitor::sync_history_progress(MpvProgressParams {
            title,
            path,
            duration_secs: Some(1480),
            watched_secs: 6,
            position_secs: 6,
            intervals: &intervals,
        });
        let records = crate::mal::AnimeSyncHistory::load_all().expect("records loaded");
        let saijo = records
            .iter()
            .find(|r| r.canonical_title == "Saijo no Osewa")
            .expect("record found");
        assert_eq!(saijo.episode, 10);
        assert_eq!(saijo.duration_secs, 1480);
        assert_eq!(saijo.watched_secs, 6);
        assert_eq!(saijo.position_secs, Some(6));
        assert_eq!(saijo.intervals.len(), 1);
        assert_eq!(saijo.status, crate::mal::SyncStatus::Watching);
    }
}
