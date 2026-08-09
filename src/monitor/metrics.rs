use anyhow::Result;
use chrono::Local;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use sysinfo::{CpuRefreshKind, ProcessRefreshKind, System};
use tokio::time::{sleep, Duration};

use crate::logger::{ActivityEvent, LogWriter};

pub struct MetricsSnapshot {
    pub cpu_pct: f32,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub ram_pct: f32,
    pub top_apps: Vec<String>,
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
        sys.refresh_processes_specifics(ProcessRefreshKind::new());

        let cpu_pct = sys.global_cpu_info().cpu_usage();
        let total_mem = sys.total_memory() / 1024 / 1024;
        let used_mem = sys.used_memory() / 1024 / 1024;
        let ram_pct = if total_mem > 0 {
            (used_mem as f32 / total_mem as f32) * 100.0
        } else {
            0.0
        };

        // Find top 3 memory consuming non-system processes
        let mut processes: Vec<_> = sys.processes().values().collect();
        processes.sort_by_key(|p| std::cmp::Reverse(p.memory()));

        let top_apps: Vec<String> = processes
            .iter()
            .take(3)
            .map(|p| {
                format!(
                    "{} ({} MB)",
                    p.name(),
                    p.memory() / 1024 / 1024
                )
            })
            .collect();

        MetricsSnapshot {
            cpu_pct,
            ram_used_mb: used_mem,
            ram_total_mb: total_mem,
            ram_pct,
            top_apps,
        }
    }

    pub async fn run(&mut self, running: Arc<AtomicBool>) -> Result<()> {
        let mut sys = System::new_all();
        // Warm up sysinfo CPU metrics calculation
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
