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
    },
    SystemMetrics {
        timestamp: String,
        cpu_pct: f32,
        ram_used_mb: u64,
        ram_total_mb: u64,
        ram_pct: f32,
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
}

impl ActivityEvent {
    pub fn timestamp(&self) -> &str {
        match self {
            ActivityEvent::WindowFocus { timestamp, .. } => timestamp,
            ActivityEvent::PowerState { timestamp, .. } => timestamp,
            ActivityEvent::SystemMetrics { timestamp, .. } => timestamp,
            ActivityEvent::MediaPlayback { timestamp, .. } => timestamp,
        }
    }
}
