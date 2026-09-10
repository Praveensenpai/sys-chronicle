use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaybackInterval {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug, Default)]
pub struct UniqueTimelineTracker {
    intervals: Vec<PlaybackInterval>,
    current_media: Option<String>,
    duration_secs: Option<u64>,
    last_position: Option<u64>,
    threshold_triggered: bool,
}

impl UniqueTimelineTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intervals(&self) -> &[PlaybackInterval] {
        &self.intervals
    }

    pub fn load_intervals(&mut self, intervals: &[PlaybackInterval]) {
        self.intervals = intervals.to_vec();
        self.merge_intervals();
    }

    pub fn set_media(&mut self, media_key: &str, duration: Option<u64>) {
        if self.current_media.as_deref() != Some(media_key) {
            self.intervals.clear();
            self.current_media = Some(media_key.to_string());
            self.duration_secs = duration;
            self.last_position = None;
            self.threshold_triggered = false;
        } else if self.duration_secs.is_none() && duration.is_some() {
            self.duration_secs = duration;
        }
    }

    pub fn set_duration(&mut self, duration: u64) {
        if duration > 0 {
            self.duration_secs = Some(duration);
        }
    }

    pub fn record_position(&mut self, current_pos: u64, is_paused: bool) {
        if is_paused {
            self.last_position = Some(current_pos);
            return;
        }

        if let Some(prev_pos) = self.last_position {
            // Normal forward playback without seeking (up to 3 seconds forward)
            if current_pos > prev_pos && current_pos <= prev_pos + 3 {
                self.add_interval(prev_pos, current_pos);
            }
        }

        self.last_position = Some(current_pos);
    }

    pub fn add_interval(&mut self, start: u64, end: u64) {
        if start >= end {
            return;
        }

        self.intervals.push(PlaybackInterval { start, end });
        self.merge_intervals();
    }

    fn merge_intervals(&mut self) {
        if self.intervals.len() <= 1 {
            return;
        }

        self.intervals.sort_by_key(|i| i.start);

        let mut merged: Vec<PlaybackInterval> = Vec::with_capacity(self.intervals.len());
        for interval in &self.intervals {
            if let Some(last) = merged.last_mut() {
                if interval.start <= last.end {
                    last.end = last.end.max(interval.end);
                    continue;
                }
            }
            merged.push(*interval);
        }

        self.intervals = merged;
    }

    pub fn total_unique_secs(&self) -> u64 {
        self.intervals
            .iter()
            .map(|i| i.end.saturating_sub(i.start))
            .sum()
    }

    pub fn coverage_pct(&self) -> f32 {
        let Some(duration) = self.duration_secs else {
            return 0.0;
        };

        if duration == 0 {
            return 0.0;
        }

        let unique = self.total_unique_secs();
        if unique >= duration.saturating_sub(3) || (unique as f64 / duration as f64) >= 0.99 {
            return 100.0;
        }

        let pct = ((unique as f64 / duration as f64) * 100.0) as f32;
        pct.min(100.0)
    }

    pub fn check_threshold(&mut self, threshold_pct: f32) -> Option<(u64, u64, f32)> {
        if self.threshold_triggered {
            return None;
        }

        let duration = self.duration_secs?;
        if duration == 0 {
            return None;
        }

        let pct = self.coverage_pct();
        if pct >= threshold_pct {
            self.threshold_triggered = true;
            let watched = if pct >= 100.0 {
                duration
            } else {
                self.total_unique_secs()
            };
            Some((duration, watched, pct))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unique_interval_merging() {
        let mut tracker = UniqueTimelineTracker::new();
        tracker.set_media("anime_ep1.mkv", Some(100));

        tracker.add_interval(0, 10);
        tracker.add_interval(5, 15);
        assert_eq!(tracker.total_unique_secs(), 15);

        tracker.add_interval(20, 30);
        assert_eq!(tracker.total_unique_secs(), 25);
    }

    #[test]
    fn test_scrubbing_does_not_trigger_threshold() {
        let mut tracker = UniqueTimelineTracker::new();
        tracker.set_media("anime_ep1.mkv", Some(100));

        // Play 2 seconds
        tracker.record_position(0, false);
        tracker.record_position(2, false);

        // Scrub to 90 seconds
        tracker.record_position(90, false);
        // Play 2 more seconds at the end
        tracker.record_position(92, false);

        assert_eq!(tracker.total_unique_secs(), 4);
        assert_eq!(tracker.coverage_pct(), 4.0);
        assert!(tracker.check_threshold(80.0).is_none());
    }

    #[test]
    fn test_sequential_playback_triggers_threshold_once() {
        let mut tracker = UniqueTimelineTracker::new();
        tracker.set_media("anime_ep1.mkv", Some(100));

        for pos in 0..=80 {
            tracker.record_position(pos, false);
        }

        assert_eq!(tracker.total_unique_secs(), 80);
        assert_eq!(tracker.coverage_pct(), 80.0);

        let result = tracker.check_threshold(80.0);
        assert!(result.is_some());
        let (duration, watched, pct) = result.expect("threshold expected");
        assert_eq!(duration, 100);
        assert_eq!(watched, 80);
        assert!((pct - 80.0).abs() < 0.01);

        // Second check should be None (fires once)
        assert!(tracker.check_threshold(80.0).is_none());
    }

    #[test]
    fn test_looping_scene_does_not_inflate_unique_time() {
        let mut tracker = UniqueTimelineTracker::new();
        tracker.set_media("anime_ep1.mkv", Some(100));

        // User loops scene 10..20 ten times
        for _ in 0..10 {
            for pos in 10..=20 {
                tracker.record_position(pos, false);
            }
        }

        assert_eq!(tracker.total_unique_secs(), 10);
        assert_eq!(tracker.coverage_pct(), 10.0);
    }

    #[test]
    fn test_seek_backward_preserves_unique_watch_duration() {
        let mut tracker = UniqueTimelineTracker::new();
        tracker.set_media("anime_ep1.mkv", Some(100));

        // Watch 0s to 15s
        for pos in 0..=15 {
            tracker.record_position(pos, false);
        }
        assert_eq!(tracker.total_unique_secs(), 15);

        // Seek back to 1s
        tracker.record_position(1, false);
        assert_eq!(tracker.total_unique_secs(), 15);

        // Rewatch 1s to 10s (already watched segment)
        for pos in 1..=10 {
            tracker.record_position(pos, false);
        }
        assert_eq!(tracker.total_unique_secs(), 15);

        // Watch new segment from 15s to 25s
        tracker.record_position(15, false);
        for pos in 16..=25 {
            tracker.record_position(pos, false);
        }
        assert_eq!(tracker.total_unique_secs(), 25);
    }

    #[test]
    fn test_load_intervals_restores_coverage() {
        let mut tracker = UniqueTimelineTracker::new();
        tracker.set_media("anime_ep1.mkv", Some(100));

        let saved = vec![PlaybackInterval { start: 0, end: 15 }];
        tracker.load_intervals(&saved);
        assert_eq!(tracker.total_unique_secs(), 15);

        // Continue watching from 15 to 20
        tracker.record_position(15, false);
        for pos in 16..=20 {
            tracker.record_position(pos, false);
        }
        assert_eq!(tracker.total_unique_secs(), 20);
    }
}
