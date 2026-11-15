use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActivityEvent {
    WindowFocus {
        timestamp: String,
        app_class: String,
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_secs: Option<u64>,
    },
    PowerState {
        timestamp: String,
        status: String,
        capacity: u8,
        ac_online: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        health_pct: Option<f32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cycle_count: Option<u32>,
    },
    SystemMetrics {
        timestamp: String,
        cpu_pct: f32,
        ram_used_mb: u64,
        ram_total_mb: u64,
        ram_pct: f32,
        /// Storage fields default to zero so logs written before disk
        /// monitoring existed still deserialize.
        #[serde(default)]
        disk_used_gb: u64,
        #[serde(default)]
        disk_total_gb: u64,
        #[serde(default)]
        disk_pct: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        disk_label: Option<String>,
        top_apps: Vec<String>,
    },
    MediaPlayback {
        timestamp: String,
        player: String,
        event_type: String,
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        position_secs: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_secs: Option<u64>,
    },
    MediaWatchThreshold {
        timestamp: String,
        player: String,
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        duration_secs: u64,
        watched_secs: u64,
        watch_pct: f32,
    },
}

impl ActivityEvent {
    pub fn timestamp(&self) -> &str {
        match self {
            ActivityEvent::WindowFocus { timestamp, .. } => timestamp,
            ActivityEvent::PowerState { timestamp, .. } => timestamp,
            ActivityEvent::SystemMetrics { timestamp, .. } => timestamp,
            ActivityEvent::MediaPlayback { timestamp, .. } => timestamp,
            ActivityEvent::MediaWatchThreshold { timestamp, .. } => timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ActivityEvent;

    #[test]
    fn reads_legacy_metrics_written_before_disk_monitoring() {
        let legacy = r#"{"type":"system_metrics","timestamp":"2026-01-01T00:00:00.000+00:00","cpu_pct":12.5,"ram_used_mb":1024,"ram_total_mb":8192,"ram_pct":12.5,"top_apps":["firefox"]}"#;

        let Ok(ActivityEvent::SystemMetrics {
            cpu_pct,
            disk_used_gb,
            disk_total_gb,
            disk_pct,
            disk_label,
            ..
        }) = serde_json::from_str(legacy)
        else {
            panic!("legacy metrics line failed to deserialize");
        };

        assert!((cpu_pct - 12.5).abs() < f32::EPSILON);
        assert_eq!(disk_used_gb, 0);
        assert_eq!(disk_total_gb, 0);
        assert!((disk_pct - 0.0).abs() < f32::EPSILON);
        assert!(disk_label.is_none());
    }

    #[test]
    fn round_trips_metrics_with_disk_fields() {
        let event = ActivityEvent::SystemMetrics {
            timestamp: "2026-01-01T00:00:00.000+00:00".to_string(),
            cpu_pct: 10.0,
            ram_used_mb: 2048,
            ram_total_mb: 16384,
            ram_pct: 12.5,
            disk_used_gb: 100,
            disk_total_gb: 512,
            disk_pct: 19.5,
            disk_label: Some("nvme0n1p2 (ext4, SSD) on /".to_string()),
            top_apps: vec!["firefox".to_string()],
        };

        let encoded = serde_json::to_string(&event).expect("encode");
        let decoded: ActivityEvent = serde_json::from_str(&encoded).expect("decode");

        let ActivityEvent::SystemMetrics {
            disk_used_gb,
            disk_total_gb,
            disk_pct,
            disk_label,
            ..
        } = decoded
        else {
            panic!("decoded event was not a metrics event");
        };

        assert_eq!(disk_used_gb, 100);
        assert_eq!(disk_total_gb, 512);
        assert!((disk_pct - 19.5).abs() < 0.01);
        assert_eq!(disk_label.as_deref(), Some("nvme0n1p2 (ext4, SSD) on /"));
    }
}
