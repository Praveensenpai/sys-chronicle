use anyhow::Result;
use chrono::Local;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::time::{sleep, Duration};

use crate::logger::{ActivityEvent, LogWriter};

pub struct PowerState {
    pub status: String,
    pub capacity: u8,
    pub ac_online: bool,
}

pub struct PowerMonitor {
    writer: LogWriter,
    last_state: Option<(String, u8, bool)>,
}

impl PowerMonitor {
    pub fn new(writer: LogWriter) -> Self {
        Self {
            writer,
            last_state: None,
        }
    }

    pub fn read_current_state() -> Option<PowerState> {
        let sys_power = Path::new("/sys/class/power_supply");
        if !sys_power.exists() {
            return None;
        }

        let mut status = "Unknown".to_string();
        let mut capacity: u8 = 100;
        let mut ac_online = false;

        if let Ok(entries) = fs::read_dir(sys_power) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();

                if name.starts_with("BAT") {
                    if let Ok(stat) = fs::read_to_string(path.join("status")) {
                        status = stat.trim().to_string();
                    }
                    if let Ok(cap_str) = fs::read_to_string(path.join("capacity")) {
                        if let Ok(cap) = cap_str.trim().parse::<u8>() {
                            capacity = cap;
                        }
                    }
                } else if name.starts_with("AC") || name.starts_with("ADP") {
                    if let Ok(online_str) = fs::read_to_string(path.join("online")) {
                        ac_online = online_str.trim() == "1";
                    }
                }
            }
        }

        Some(PowerState {
            status,
            capacity,
            ac_online,
        })
    }

    pub async fn run(&mut self, running: Arc<AtomicBool>) -> Result<()> {
        let mut poll_count = 0u64;

        while running.load(Ordering::SeqCst) {
            if let Some(state) = Self::read_current_state() {
                let should_log = match &self.last_state {
                    Some((old_status, old_cap, old_ac)) => {
                        old_status != &state.status
                            || old_ac != &state.ac_online
                            || (old_cap.abs_diff(state.capacity) >= 2)
                            || (poll_count % 60 == 0) // Log snapshot every ~5 minutes (60 * 5s)
                    }
                    None => true,
                };

                if should_log {
                    let now = Local::now();
                    let timestamp_str = now.format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string();

                    let event = ActivityEvent::PowerState {
                        timestamp: timestamp_str,
                        status: state.status.clone(),
                        capacity: state.capacity,
                        ac_online: state.ac_online,
                    };

                    let _ = self.writer.write_event(&event);
                    self.last_state = Some((state.status, state.capacity, state.ac_online));
                }
            }

            poll_count += 1;
            sleep(Duration::from_secs(5)).await;
        }

        Ok(())
    }
}
