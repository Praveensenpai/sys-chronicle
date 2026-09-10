use chrono::Local;
use std::path::Path;

use crate::mal::history::ProgressUpdateParams;
use crate::mal::{AnimeParser, AnimeSyncHistory};
use crate::monitor::mpv::MpvMonitor;
use crate::monitor::timeline_tracker::{PlaybackInterval, UniqueTimelineTracker};

pub struct MpvProgressParams<'a> {
    pub title: &'a str,
    pub path: Option<&'a str>,
    pub duration_secs: Option<u64>,
    pub watched_secs: u64,
    pub position_secs: u64,
    pub intervals: &'a [PlaybackInterval],
}

pub struct MpvPlaybackSession {
    pub title: String,
    pub path: Option<String>,
    pub paused: bool,
    pub pos: u64,
    pub duration: Option<u64>,
    last_pos_update: chrono::DateTime<Local>,
    last_history_sync: std::time::Instant,
    started: bool,
    tracker: UniqueTimelineTracker,
}

impl Default for MpvPlaybackSession {
    fn default() -> Self {
        Self::new()
    }
}

impl MpvPlaybackSession {
    pub fn new() -> Self {
        Self {
            title: String::new(),
            path: None,
            paused: false,
            pos: 0,
            duration: None,
            last_pos_update: Local::now(),
            last_history_sync: std::time::Instant::now(),
            started: false,
            tracker: UniqueTimelineTracker::new(),
        }
    }

    pub fn transition_media(
        &mut self,
        monitor: &MpvMonitor,
        new_title: String,
        new_path: Option<String>,
    ) {
        if self.started && !self.title.is_empty() {
            monitor.log_media_event(
                "stop",
                &self.title,
                self.path.as_deref(),
                self.pos,
                self.duration,
            );
            self.sync_history();
        }

        self.title = new_title;
        self.path = new_path;
        self.pos = 0;
        self.duration = None;
        self.tracker.set_media(&self.title, None);

        let raw = self.path.as_deref().unwrap_or(&self.title);
        if let Some(info) = AnimeParser::parse(raw) {
            if let Some(existing) = AnimeSyncHistory::find_record(raw, &info.title, info.episode) {
                if existing.episode == info.episode && !existing.intervals.is_empty() {
                    self.tracker.load_intervals(&existing.intervals);
                }
            }
        }

        monitor.log_media_event(
            "start",
            &self.title,
            self.path.as_deref(),
            self.pos,
            self.duration,
        );
        self.sync_history();
        self.started = true;
    }

    pub fn update_title(&mut self, monitor: &MpvMonitor, new_title: String) {
        if new_title.is_empty() || new_title == self.title {
            return;
        }
        let matching_path = self.path.as_ref().and_then(|p| {
            let fname = Path::new(p).file_name()?.to_string_lossy();
            if fname == new_title {
                Some(p.clone())
            } else {
                None
            }
        });
        self.transition_media(monitor, new_title, matching_path);
    }

    pub fn update_path(&mut self, monitor: &MpvMonitor, new_path_str: String) {
        let file_name = Path::new(&new_path_str)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        if self.path.as_deref() == Some(&new_path_str) {
            return;
        }

        if !self.title.is_empty() && (self.title == file_name || self.path.is_none()) {
            self.path = Some(new_path_str);
            self.sync_history();
        } else {
            let title = if file_name.is_empty() {
                new_path_str.clone()
            } else {
                file_name
            };
            self.transition_media(monitor, title, Some(new_path_str));
        }
    }

    pub fn update_pause(&mut self, monitor: &MpvMonitor, paused: bool) {
        if paused != self.paused {
            self.paused = paused;
            let action = if paused { "pause" } else { "resume" };
            monitor.log_media_event(
                action,
                &self.title,
                self.path.as_deref(),
                self.pos,
                self.duration,
            );
            self.sync_history();
        }
    }

    pub fn update_time_pos(&mut self, monitor: &MpvMonitor, pos_num: f64) {
        let new_pos = pos_num.max(0.0) as u64;
        let now = Local::now();
        let elapsed_wall = (now - self.last_pos_update).num_seconds().max(0) as u64;

        if self.started && !self.paused {
            let expected_pos = self.pos + elapsed_wall;
            if (new_pos as i64 - expected_pos as i64).abs() > 5 {
                let action = if new_pos > self.pos {
                    "seek_forward"
                } else {
                    "seek_backward"
                };
                monitor.log_media_event(
                    action,
                    &self.title,
                    self.path.as_deref(),
                    new_pos,
                    self.duration,
                );
            }
        }

        self.tracker.record_position(new_pos, self.paused);
        if let Some((dur, watched, pct)) = self.tracker.check_threshold(80.0) {
            monitor.log_threshold_event(&self.title, self.path.as_deref(), dur, watched, pct);
        }

        if self.last_history_sync.elapsed() >= std::time::Duration::from_secs(3) {
            self.sync_history();
            self.last_history_sync = std::time::Instant::now();
        }

        self.pos = new_pos;
        self.last_pos_update = now;
    }

    pub fn update_duration(&mut self, dur_num: f64) {
        let dur = dur_num.max(0.0) as u64;
        self.duration = Some(dur);
        self.tracker.set_duration(dur);
        self.sync_history();
    }

    pub fn finish(&mut self, monitor: &MpvMonitor) {
        if self.started && !self.title.is_empty() {
            monitor.log_media_event(
                "stop",
                &self.title,
                self.path.as_deref(),
                self.pos,
                self.duration,
            );
            self.sync_history();
        }
    }

    pub fn sync_history(&self) {
        Self::sync_history_progress(MpvProgressParams {
            title: &self.title,
            path: self.path.as_deref(),
            duration_secs: self.duration,
            watched_secs: self.tracker.total_unique_secs(),
            position_secs: self.pos,
            intervals: self.tracker.intervals(),
        });
    }

    pub fn sync_history_progress(params: MpvProgressParams<'_>) {
        let raw = params.path.unwrap_or(params.title);
        if let Some(info) = AnimeParser::parse(raw) {
            let duration = params.duration_secs.unwrap_or(0);
            if duration > 0 {
                let _ = AnimeSyncHistory::update_progress_full(ProgressUpdateParams {
                    raw_title: raw,
                    canonical_title: &info.title,
                    episode: info.episode,
                    path: params.path.map(|s| s.to_string()),
                    duration_secs: duration,
                    watched_secs: params.watched_secs,
                    position_secs: Some(params.position_secs),
                    intervals: params.intervals,
                });
            }
        }
    }
}
