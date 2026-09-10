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

use super::mpv_session::MpvPlaybackSession;

impl MpvMonitor {
    pub fn new(writer: LogWriter) -> Self {
        Self { writer }
    }

    fn candidate_sockets() -> Vec<PathBuf> {
        let mut paths = vec![PathBuf::from("/tmp/mpvsocket")];
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
        Self::candidate_sockets().into_iter().find(|p| p.exists())
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

        let mut session = MpvPlaybackSession::new();

        while running.load(Ordering::SeqCst) {
            tokio::select! {
                line = lines.next_line() => {
                    match line {
                        Ok(Some(raw_line)) => {
                            if let Ok(msg) = serde_json::from_str::<MpvEvent>(&raw_line) {
                                if msg.event.as_deref() == Some("property-change") {
                                    self.dispatch_property_change(&mut session, msg);
                                }
                            }
                        }
                        Ok(None) | Err(_) => break,
                    }
                }
                _ = sleep(Duration::from_secs(1)) => {}
            }
        }

        session.finish(self);
        Ok(())
    }

    fn dispatch_property_change(&self, session: &mut MpvPlaybackSession, msg: MpvEvent) {
        match msg.id {
            Some(1) => {
                if let Some(val) = msg.data.as_ref().and_then(|v| v.as_str()) {
                    session.update_title(self, val.trim().to_string());
                }
            }
            Some(2) => {
                if let Some(val) = msg.data.as_ref().and_then(|v| v.as_str()) {
                    session.update_path(self, val.to_string());
                }
            }
            Some(3) => {
                if let Some(paused) = msg.data.as_ref().and_then(|v| v.as_bool()) {
                    session.update_pause(self, paused);
                }
            }
            Some(4) => {
                if let Some(pos) = msg.data.as_ref().and_then(|v| v.as_f64()) {
                    session.update_time_pos(self, pos);
                }
            }
            Some(5) => {
                if let Some(dur) = msg.data.as_ref().and_then(|v| v.as_f64()) {
                    session.update_duration(dur);
                }
            }
            _ => {}
        }
    }

    fn dispatch_event(&self, event: ActivityEvent) {
        let _ = self.writer.write_event(&event);
        crate::plugin::PluginDispatcher::dispatch_event(&event);
    }

    pub(crate) fn log_media_event(
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
        self.dispatch_event(ActivityEvent::MediaPlayback {
            timestamp: Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string(),
            player: "mpv".to_string(),
            event_type: event_type.to_string(),
            title: title.to_string(),
            path: path.map(|p| p.to_string()),
            position_secs,
            duration_secs,
        });
    }

    pub(crate) fn log_threshold_event(
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
        self.dispatch_event(ActivityEvent::MediaWatchThreshold {
            timestamp: Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string(),
            player: "mpv".to_string(),
            title: title.to_string(),
            path: path.map(|p| p.to_string()),
            duration_secs,
            watched_secs,
            watch_pct,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::mpv_session::MpvProgressParams;
    use super::*;

    #[test]
    fn test_sync_history_progress_creates_record() {
        let title = "Saijo no Osewa - 10.mkv";
        let path = Some("/home/paisen/Videos/Anime/Saijo no Osewa - 10.mkv");
        let intervals = vec![crate::monitor::PlaybackInterval { start: 0, end: 6 }];
        MpvPlaybackSession::sync_history_progress(MpvProgressParams {
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
