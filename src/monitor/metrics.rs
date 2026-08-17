use anyhow::Result;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use sysinfo::{CpuRefreshKind, ProcessRefreshKind, System};
use tokio::time::{sleep, Duration};

use crate::logger::{ActivityEvent, LogWriter};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppDetail {
    pub name: String,
    pub exe_path: String,
    pub process_count: usize,
    pub ram_mb: u64,
    pub ram_pct: f32,
    pub cpu_pct: f32,
}

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

struct AppAcc {
    exe_path: String,
    process_count: usize,
    ram_mb: u64,
    cpu_pct: f32,
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

        // Group process physical RSS memory, CPU %, and process count by clean executable name
        let mut app_map: BTreeMap<String, AppAcc> = BTreeMap::new();
        let mut seen_tgids = HashSet::new();

        for (pid, p) in sys.processes() {
            if p.cmd().is_empty() {
                continue;
            }

            let pid_u32 = pid.as_u32();

            // Read PID, TGID, and VmRSS from /proc/[pid]/status
            if let Some((proc_pid, proc_tgid, rss_kb)) = Self::get_proc_status_info(pid_u32) {
                // Skip sub-threads (Pid != Tgid)
                if proc_pid != proc_tgid {
                    continue;
                }

                // Deduplicate main process TGIDs
                if seen_tgids.contains(&proc_tgid) {
                    continue;
                }
                seen_tgids.insert(proc_tgid);

                let raw_exe_path = p.exe().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();

                let exe_name = if let Some(exe) = p.exe() {
                    if let Some(file_name) = exe.file_name() {
                        let name = file_name.to_string_lossy().to_string();
                        if !name.is_empty() {
                            name
                        } else {
                            p.name().to_string()
                        }
                    } else {
                        p.name().to_string()
                    }
                } else {
                    p.name().to_string()
                };

                let clean_name = if exe_name.contains('/') || exe_name.contains('\\') {
                    std::path::Path::new(&exe_name)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or(exe_name)
                } else {
                    exe_name
                };

                let mem_mb = rss_kb / 1024;
                let proc_cpu = p.cpu_usage();

                let entry = app_map.entry(clean_name).or_insert(AppAcc {
                    exe_path: raw_exe_path.clone(),
                    process_count: 0,
                    ram_mb: 0,
                    cpu_pct: 0.0,
                });

                entry.process_count += 1;
                entry.ram_mb += mem_mb;
                entry.cpu_pct += proc_cpu;
                if entry.exe_path.is_empty() && !raw_exe_path.is_empty() {
                    entry.exe_path = raw_exe_path;
                }
            }
        }

        let mut sorted_apps: Vec<(String, AppAcc)> = app_map.into_iter().collect();
        sorted_apps.sort_by_key(|(_, acc)| std::cmp::Reverse(acc.ram_mb));

        let cpu_count = sys.cpus().len().max(1) as f32;

        let app_details: Vec<AppDetail> = sorted_apps
            .into_iter()
            .map(|(name, acc)| {
                let app_ram_pct = if total_mem > 0 {
                    (acc.ram_mb as f32 / total_mem as f32) * 100.0
                } else {
                    0.0
                };
                AppDetail {
                    name: name.clone(),
                    exe_path: if acc.exe_path.is_empty() { name.clone() } else { acc.exe_path.clone() },
                    process_count: acc.process_count,
                    ram_mb: acc.ram_mb,
                    ram_pct: app_ram_pct,
                    cpu_pct: (acc.cpu_pct / cpu_count).clamp(0.0, 100.0),
                }
            })
            .collect();

        let top_apps: Vec<String> = app_details
            .iter()
            .take(4)
            .map(|app| format!("{} ({} MB)", app.name, app.ram_mb))
            .collect();

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
                if let Some(millidegrees) = Self::read_number(path.join(format!("temp{index}_input"))) {
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
        fs::read_to_string(path).ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
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

    fn get_proc_status_info(pid: u32) -> Option<(u32, u32, u64)> {
        let status_path = format!("/proc/{}/status", pid);
        let content = fs::read_to_string(status_path).ok()?;
        let mut proc_pid = None;
        let mut proc_tgid = None;
        let mut rss_kb = 0u64;

        for line in content.lines() {
            if line.starts_with("Pid:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    proc_pid = parts[1].parse::<u32>().ok();
                }
            } else if line.starts_with("Tgid:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    proc_tgid = parts[1].parse::<u32>().ok();
                }
            } else if line.starts_with("VmRSS:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    rss_kb = parts[1].parse::<u64>().unwrap_or(0);
                }
            }
        }

        match (proc_pid, proc_tgid) {
            (Some(pid_val), Some(tgid_val)) => Some((pid_val, tgid_val, rss_kb)),
            _ => None,
        }
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
