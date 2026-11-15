use crate::logger::ActivityEvent;

use super::format::extract_short_time;

/// Storage sample: used gigabytes, total gigabytes and used percentage.
type DiskSample = (u64, u64, f32);

/// Resource readings of a single metrics event.
struct SpikeSample<'a> {
    timestamp: &'a str,
    cpu_pct: f32,
    ram_pct: f32,
    ram_used_mb: u64,
    ram_total_mb: u64,
    disk_pct: f32,
    disk_used_gb: u64,
    disk_total_gb: u64,
    top_apps: &'a [String],
}

/// Builds the resource utilization section of the report.
pub fn resource_section(events: &[ActivityEvent]) -> String {
    let mut report = String::new();
    report.push_str("## 5. CPU, RAM & Disk Resource Utilization\n");

    let mut cpu_samples = Vec::new();
    let mut ram_samples = Vec::new();
    let mut disk_samples: Vec<DiskSample> = Vec::new();
    let mut disk_labels: Vec<String> = Vec::new();
    let mut high_load_spikes = Vec::new();

    for event in events {
        if let ActivityEvent::SystemMetrics {
            timestamp,
            cpu_pct,
            ram_used_mb,
            ram_total_mb,
            ram_pct,
            disk_used_gb,
            disk_total_gb,
            disk_pct,
            disk_label,
            top_apps,
        } = event
        {
            cpu_samples.push(*cpu_pct);
            ram_samples.push((*ram_used_mb, *ram_total_mb, *ram_pct));
            // Samples logged before disk monitoring existed carry no storage data.
            if *disk_total_gb > 0 {
                disk_samples.push((*disk_used_gb, *disk_total_gb, *disk_pct));
            }
            if let Some(volume) = disk_label {
                if !disk_labels.contains(volume) {
                    disk_labels.push(volume.clone());
                }
            }

            if *cpu_pct >= 70.0 || *ram_pct >= 85.0 || *disk_pct >= 85.0 {
                high_load_spikes.push(format_spike(&SpikeSample {
                    timestamp,
                    cpu_pct: *cpu_pct,
                    ram_pct: *ram_pct,
                    ram_used_mb: *ram_used_mb,
                    ram_total_mb: *ram_total_mb,
                    disk_pct: *disk_pct,
                    disk_used_gb: *disk_used_gb,
                    disk_total_gb: *disk_total_gb,
                    top_apps,
                }));
            }
        }
    }

    if cpu_samples.is_empty() {
        report.push_str("No system performance samples recorded.\n\n");
        return report;
    }

    report.push_str(&format_cpu_and_ram(&cpu_samples, &ram_samples));
    report.push_str(&format_disk_usage(&disk_samples, &disk_labels));

    if !high_load_spikes.is_empty() {
        report.push_str("### Resource Spikes (>70% CPU, >85% RAM or >85% Disk)\n");
        for spike in high_load_spikes.iter().take(20) {
            report.push_str(spike);
            report.push('\n');
        }
        report.push('\n');
    }

    report
}

/// Formats the CPU and RAM utilisation lines.
fn format_cpu_and_ram(cpu_samples: &[f32], ram_samples: &[(u64, u64, f32)]) -> String {
    let avg_cpu: f32 = cpu_samples.iter().sum::<f32>() / cpu_samples.len() as f32;
    let max_cpu: f32 = cpu_samples.iter().cloned().fold(0.0, f32::max);

    let avg_ram: f32 =
        ram_samples.iter().map(|(_, _, p)| p).sum::<f32>() / ram_samples.len() as f32;
    let max_ram: f32 = ram_samples.iter().map(|(_, _, p)| *p).fold(0.0, f32::max);
    let max_ram_mb = ram_samples.iter().map(|(u, _, _)| *u).max().unwrap_or(0);
    let total_ram_mb = ram_samples.first().map(|(_, t, _)| *t).unwrap_or(0);

    format!(
        "- **CPU Load**: Avg `{avg_cpu:.1}%` | Peak `{max_cpu:.1}%`\n\
         - **RAM Load**: Avg `{avg_ram:.1}%` | Peak `{max_ram:.1}%` ({max_ram_mb} / {total_ram_mb} MB)\n"
    )
}

/// Formats the storage utilisation line.
fn format_disk_usage(samples: &[DiskSample], labels: &[String]) -> String {
    if samples.is_empty() {
        return "- **Disk Usage**: Not tracked (no storage samples recorded)\n\n".to_string();
    }

    let total_gb = samples
        .iter()
        .map(|(_, total, _)| *total)
        .max()
        .unwrap_or(0);
    let avg_pct = samples.iter().map(|(_, _, pct)| *pct).sum::<f32>() / samples.len() as f32;
    let max_pct = samples.iter().map(|(_, _, pct)| *pct).fold(0.0, f32::max);
    let max_used_gb = samples.iter().map(|(used, _, _)| *used).max().unwrap_or(0);
    let volume = match labels.first() {
        Some(label) => format!(" | {label}"),
        None => String::new(),
    };

    format!(
        "- **Disk Usage**: Avg `{avg_pct:.1}%` | Peak `{max_pct:.1}%` ({max_used_gb} / {total_gb} GB){volume}\n\n"
    )
}

/// Formats a single resource spike entry.
fn format_spike(sample: &SpikeSample<'_>) -> String {
    let time_str = extract_short_time(sample.timestamp);
    let disk_detail = if sample.disk_total_gb > 0 {
        format!(
            " | Disk: **{:.1}%** ({} / {} GB)",
            sample.disk_pct, sample.disk_used_gb, sample.disk_total_gb
        )
    } else {
        String::new()
    };

    format!(
        "- `[{}]` CPU: **{:.1}%** | RAM: **{:.1}%** ({} / {} MB){} | Top Apps: {}",
        time_str,
        sample.cpu_pct,
        sample.ram_pct,
        sample.ram_used_mb,
        sample.ram_total_mb,
        disk_detail,
        sample.top_apps.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::format_disk_usage;
    use crate::logger::ActivityEvent;

    #[test]
    fn reports_disk_usage_with_volume_label() {
        let samples = vec![(150, 236, 70.0), (180, 236, 80.0)];
        let labels = vec!["mapper/root (btrfs, SSD) on /".to_string()];

        let line = format_disk_usage(&samples, &labels);

        assert!(line.contains("Avg `75.0%`"), "{line}");
        assert!(line.contains("Peak `80.0%`"), "{line}");
        assert!(line.contains("(180 / 236 GB)"), "{line}");
        assert!(line.contains("mapper/root (btrfs, SSD) on /"), "{line}");
    }

    #[test]
    fn marks_disk_as_untracked_without_samples() {
        let line = format_disk_usage(&[], &[]);

        assert!(line.contains("Not tracked"), "{line}");
        assert!(!line.contains('%'), "{line}");
    }

    #[test]
    fn report_omits_disk_detail_for_legacy_samples() {
        let legacy = ActivityEvent::SystemMetrics {
            timestamp: "2026-01-01T10:00:00.000+00:00".to_string(),
            cpu_pct: 95.0,
            ram_used_mb: 1000,
            ram_total_mb: 8000,
            ram_pct: 12.5,
            disk_used_gb: 0,
            disk_total_gb: 0,
            disk_pct: 0.0,
            disk_label: None,
            top_apps: vec!["firefox".to_string()],
        };

        let report = super::resource_section(&[legacy]);

        assert!(report.contains("CPU: **95.0%**"), "{report}");
        assert!(!report.contains("Disk: **"), "{report}");
        assert!(report.contains("Disk Usage**: Not tracked"), "{report}");
    }

    #[test]
    fn report_includes_disk_detail_for_tracked_samples() {
        let tracked = ActivityEvent::SystemMetrics {
            timestamp: "2026-01-01T10:00:00.000+00:00".to_string(),
            cpu_pct: 10.0,
            ram_used_mb: 1000,
            ram_total_mb: 8000,
            ram_pct: 12.5,
            disk_used_gb: 173,
            disk_total_gb: 236,
            disk_pct: 73.4,
            disk_label: Some("mapper/root (btrfs, SSD) on /".to_string()),
            top_apps: vec!["firefox".to_string()],
        };

        let report = super::resource_section(&[tracked]);

        assert!(report.contains("Disk Usage**: Avg `73.4%`"), "{report}");
        assert!(report.contains("(173 / 236 GB)"), "{report}");
        assert!(report.contains("mapper/root (btrfs, SSD) on /"), "{report}");
    }
}
