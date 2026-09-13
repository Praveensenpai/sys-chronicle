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
    pub health_pct: Option<f32>,
    pub cycle_count: Option<u32>,
}

fn read_u64_from_file(path: &Path) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse::<u64>().ok()
}

fn read_u32_from_file(path: &Path) -> Option<u32> {
    fs::read_to_string(path).ok()?.trim().parse::<u32>().ok()
}

fn read_battery_health_and_cycles(bat_dir: &Path) -> (Option<f32>, Option<u32>) {
    let energy_full = read_u64_from_file(&bat_dir.join("energy_full"));
    let energy_design = read_u64_from_file(&bat_dir.join("energy_full_design"));
    let charge_full = read_u64_from_file(&bat_dir.join("charge_full"));
    let charge_design = read_u64_from_file(&bat_dir.join("charge_full_design"));
    let cycle_count = read_u32_from_file(&bat_dir.join("cycle_count"));

    let health_pct = match (energy_full, energy_design) {
        (Some(full), Some(design)) if design > 0 => {
            Some(((full as f64 / design as f64) * 100.0).clamp(0.0, 100.0) as f32)
        }
        _ => match (charge_full, charge_design) {
            (Some(full), Some(design)) if design > 0 => {
                Some(((full as f64 / design as f64) * 100.0).clamp(0.0, 100.0) as f32)
            }
            _ => None,
        },
    };

    (health_pct, cycle_count)
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
        let mut health_pct = None;
        let mut cycle_count = None;

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
                    let (h, c) = read_battery_health_and_cycles(&path);
                    health_pct = h;
                    cycle_count = c;
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
            health_pct,
            cycle_count,
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
                            || poll_count.is_multiple_of(60) // Log snapshot every ~5 minutes (60 * 5s)
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
                        health_pct: state.health_pct,
                        cycle_count: state.cycle_count,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_battery_health_energy() {
        let test_dir = std::env::temp_dir().join("test_battery_health_energy");
        let _ = fs::remove_dir_all(&test_dir);
        let _ = fs::create_dir_all(&test_dir);

        let _ = fs::write(test_dir.join("energy_full"), "27349000\n");
        let _ = fs::write(test_dir.join("energy_full_design"), "42067000\n");
        let _ = fs::write(test_dir.join("cycle_count"), "819\n");

        let (health, cycles) = read_battery_health_and_cycles(&test_dir);
        assert!(health.is_some());
        let h = health.unwrap_or(0.0);
        assert!((h - 65.01).abs() < 0.1);
        assert_eq!(cycles, Some(819));

        let _ = fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_read_battery_health_charge() {
        let test_dir = std::env::temp_dir().join("test_battery_health_charge");
        let _ = fs::remove_dir_all(&test_dir);
        let _ = fs::create_dir_all(&test_dir);

        let _ = fs::write(test_dir.join("charge_full"), "4000000\n");
        let _ = fs::write(test_dir.join("charge_full_design"), "5000000\n");

        let (health, cycles) = read_battery_health_and_cycles(&test_dir);
        assert!(health.is_some());
        let h = health.unwrap_or(0.0);
        assert!((h - 80.0).abs() < 0.1);
        assert_eq!(cycles, None);

        let _ = fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_read_battery_health_empty() {
        let test_dir = std::env::temp_dir().join("test_battery_health_empty");
        let _ = fs::remove_dir_all(&test_dir);
        let _ = fs::create_dir_all(&test_dir);

        let (health, cycles) = read_battery_health_and_cycles(&test_dir);
        assert!(health.is_none());
        assert!(cycles.is_none());

        let _ = fs::remove_dir_all(&test_dir);
    }
}
