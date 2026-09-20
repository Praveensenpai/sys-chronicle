use anyhow::Result;
use chrono::Local;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use sysinfo::{CpuRefreshKind, ProcessRefreshKind, System};
use tokio::time::{sleep, Duration};

use super::process::collect_app_details;
pub use super::process::AppDetail;
use crate::logger::{ActivityEvent, LogWriter};

pub struct MetricsSnapshot {
    pub cpu_pct: f32,
    pub cpu_temp_c: Option<f32>,
    pub cpu_temp_label: Option<String>,
    pub fan_rpm: Option<u32>,
    pub fan_label: Option<String>,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub ram_pct: f32,
    pub top_apps: Vec<String>,
    pub app_details: Vec<AppDetail>,
}

pub struct MetricsMonitor {
    writer: LogWriter,
    interval_secs: u64,
}

impl MetricsMonitor {
    pub fn new(writer: LogWriter, interval_secs: u64) -> Self {
        Self {
            writer,
            interval_secs,
        }
    }

    pub fn sample_metrics(sys: &mut System) -> MetricsSnapshot {
        sys.refresh_cpu_specifics(CpuRefreshKind::everything());
        sys.refresh_memory();
        sys.refresh_processes_specifics(ProcessRefreshKind::everything());

        let cpu_pct = sys.global_cpu_info().cpu_usage();
        let (cpu_temp_c, cpu_temp_label, fan_rpm, fan_label) = Self::read_hardware_sensors();
        let total_mem = sys.total_memory() / (1024 * 1024);
        let used_mem = sys.used_memory() / (1024 * 1024);
        let ram_pct = if total_mem > 0 {
            (used_mem as f32 / total_mem as f32) * 100.0
        } else {
            0.0
        };

        let (app_details, top_apps) = collect_app_details(sys, total_mem);

        MetricsSnapshot {
            cpu_pct,
            cpu_temp_c,
            cpu_temp_label,
            fan_rpm,
            fan_label,
            ram_used_mb: used_mem,
            ram_total_mb: total_mem,
            ram_pct,
            top_apps,
            app_details,
        }
    }

    /// Reads Linux hwmon values directly because fan speeds are not exposed by sysinfo.
    /// Missing or inaccessible sensors are normal on many laptops and virtual machines.
    fn read_hardware_sensors() -> (Option<f32>, Option<String>, Option<u32>, Option<String>) {
        let Ok(entries) = fs::read_dir("/sys/class/hwmon") else {
            return (None, None, None, None);
        };

        let mut cpu_temperatures: Vec<(f32, String)> = Vec::new();
        let mut fallback_temperatures: Vec<(f32, String)> = Vec::new();
        let mut cpu_fans: Vec<(u32, String)> = Vec::new();
        let mut fallback_fans: Vec<(u32, String)> = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            let device_name = Self::read_text(path.join("name")).unwrap_or_else(|| "sensor".into());
            let source_is_cpu = matches!(
                device_name.as_str(),
                "coretemp" | "k10temp" | "zenpower" | "cpu_thermal" | "x86_pkg_temp"
            );

            for index in 1..=16 {
                let label = Self::read_text(path.join(format!("temp{index}_label")))
                    .unwrap_or_else(|| format!("{device_name} temp"));
                let is_cpu_label = Self::is_cpu_sensor_label(&label);
                if let Some(millidegrees) =
                    Self::read_number(path.join(format!("temp{index}_input")))
                {
                    let reading = (millidegrees as f32 / 1000.0, label);
                    if source_is_cpu || is_cpu_label {
                        cpu_temperatures.push(reading);
                    } else {
                        fallback_temperatures.push(reading);
                    }
                }

                let fan_label = Self::read_text(path.join(format!("fan{index}_label")))
                    .unwrap_or_else(|| format!("{device_name} fan"));
                let is_cpu_fan = source_is_cpu || Self::is_cpu_sensor_label(&fan_label);
                if let Some(rpm) = Self::read_number(path.join(format!("fan{index}_input"))) {
                    let reading = (rpm, fan_label);
                    if is_cpu_fan {
                        cpu_fans.push(reading);
                    } else {
                        fallback_fans.push(reading);
                    }
                }
            }
        }

        // The package sensor is usually the best CPU reading; take the hottest core if needed.
        cpu_temperatures.sort_by(|a, b| b.0.total_cmp(&a.0));
        fallback_temperatures.sort_by(|a, b| b.0.total_cmp(&a.0));
        cpu_fans.sort_by_key(|reading| std::cmp::Reverse(reading.0));
        fallback_fans.sort_by_key(|reading| std::cmp::Reverse(reading.0));

        let temperature = cpu_temperatures
            .into_iter()
            .next()
            .or_else(|| fallback_temperatures.into_iter().next());
        let fan = cpu_fans
            .into_iter()
            .next()
            .or_else(|| fallback_fans.into_iter().next());

        (
            temperature.as_ref().map(|(value, _)| *value),
            temperature.map(|(_, label)| label),
            fan.as_ref().map(|(value, _)| *value),
            fan.map(|(_, label)| label),
        )
    }

    fn read_text(path: impl AsRef<Path>) -> Option<String> {
        fs::read_to_string(path)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    fn read_number(path: impl AsRef<Path>) -> Option<u32> {
        Self::read_text(path)?.parse().ok()
    }

    fn is_cpu_sensor_label(label: &str) -> bool {
        let lower = label.to_ascii_lowercase();
        lower.contains("package")
            || lower.contains("cpu")
            || lower.contains("tdie")
            || lower.contains("tctl")
            || lower.contains("core")
    }

    pub async fn run(&mut self, running: Arc<AtomicBool>) -> Result<()> {
        let mut sys = System::new_all();
        sys.refresh_cpu_specifics(CpuRefreshKind::everything());
        sleep(Duration::from_millis(500)).await;

        while running.load(Ordering::SeqCst) {
            let metrics = Self::sample_metrics(&mut sys);
            let now = Local::now();
            let timestamp_str = now.format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string();

            let event = ActivityEvent::SystemMetrics {
                timestamp: timestamp_str,
                cpu_pct: metrics.cpu_pct,
                ram_used_mb: metrics.ram_used_mb,
                ram_total_mb: metrics.ram_total_mb,
                ram_pct: metrics.ram_pct,
                top_apps: metrics.top_apps,
            };

            let _ = self.writer.write_event(&event);

            sleep(Duration::from_secs(self.interval_secs)).await;
        }

        Ok(())
    }
}
