use std::ffi::OsStr;

use sysinfo::{Disk, Disks};

/// Bytes contained in one gibibyte.
const BYTES_PER_GIB: u64 = 1024 * 1024 * 1024;

/// Mount point of the primary filesystem.
const ROOT_MOUNT_POINT: &str = "/";

/// Prefix stripped from raw device names for display purposes.
const DEVICE_PREFIX: &str = "/dev/";

/// Capacity snapshot of the filesystem backing the root mount.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DiskSnapshot {
    pub device: String,
    pub mount_point: String,
    pub file_system: String,
    pub media_type: String,
    pub used_gb: u64,
    pub total_gb: u64,
    pub used_pct: f32,
}

impl DiskSnapshot {
    /// Short description of the volume, for example `nvme0n1p2 (ext4, SSD) on /`.
    pub fn label(&self) -> String {
        let media = if self.media_type.is_empty() {
            String::new()
        } else {
            format!(", {}", self.media_type)
        };
        format!(
            "{} ({}{}) on {}",
            self.device, self.file_system, media, self.mount_point
        )
    }
}

/// Flat view of a mounted volume, independent of the sysinfo representation.
#[derive(Debug, Clone, PartialEq)]
struct DiskEntry {
    device: String,
    mount_point: String,
    file_system: String,
    media_type: String,
    total_bytes: u64,
    available_bytes: u64,
}

impl From<&Disk> for DiskEntry {
    fn from(disk: &Disk) -> Self {
        Self {
            device: device_name(disk.name()),
            mount_point: disk.mount_point().to_string_lossy().to_string(),
            file_system: disk.file_system().to_string_lossy().to_string(),
            media_type: disk.kind().to_string(),
            total_bytes: disk.total_space(),
            available_bytes: disk.available_space(),
        }
    }
}

/// Samples capacity of the filesystem backing the root mount.
///
/// Returns `None` when the host exposes no usable volume at all.
pub fn sample_primary_disk(disks: &mut Disks) -> Option<DiskSnapshot> {
    disks.refresh_list();
    let entries: Vec<DiskEntry> = disks.list().iter().map(DiskEntry::from).collect();
    select_root(&entries).map(snapshot)
}

/// Picks the volume backing the root mount, falling back to the largest
/// volume when `/` is not exposed as a mount point.
fn select_root(entries: &[DiskEntry]) -> Option<&DiskEntry> {
    entries
        .iter()
        .find(|entry| entry.mount_point == ROOT_MOUNT_POINT)
        .or_else(|| entries.iter().max_by_key(|entry| entry.total_bytes))
}

/// Builds the public snapshot for a volume.
fn snapshot(entry: &DiskEntry) -> DiskSnapshot {
    let used_bytes = entry.total_bytes.saturating_sub(entry.available_bytes);
    DiskSnapshot {
        device: entry.device.clone(),
        mount_point: entry.mount_point.clone(),
        file_system: entry.file_system.clone(),
        media_type: entry.media_type.clone(),
        used_gb: used_bytes / BYTES_PER_GIB,
        total_gb: entry.total_bytes / BYTES_PER_GIB,
        used_pct: usage_pct(used_bytes, entry.total_bytes),
    }
}

/// Percentage of the volume that is in use.
fn usage_pct(used_bytes: u64, total_bytes: u64) -> f32 {
    if total_bytes == 0 {
        return 0.0;
    }
    (used_bytes as f64 / total_bytes as f64 * 100.0) as f32
}

/// Renders a device name without the `/dev/` prefix.
fn device_name(raw: &OsStr) -> String {
    let name = raw.to_string_lossy();
    name.strip_prefix(DEVICE_PREFIX)
        .unwrap_or(&name)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(mount_point: &str, total_gb: u64, available_gb: u64) -> DiskEntry {
        DiskEntry {
            device: "nvme0n1p2".to_string(),
            mount_point: mount_point.to_string(),
            file_system: "ext4".to_string(),
            media_type: "SSD".to_string(),
            total_bytes: total_gb * BYTES_PER_GIB,
            available_bytes: available_gb * BYTES_PER_GIB,
        }
    }

    #[test]
    fn selects_the_root_mount_over_larger_volumes() {
        let entries = vec![entry("/home", 4096, 1000), entry("/", 512, 100)];
        let selected = select_root(&entries).expect("root mount");
        assert_eq!(selected.mount_point, "/");
    }

    #[test]
    fn falls_back_to_the_largest_volume_when_root_is_absent() {
        let entries = vec![entry("/home", 256, 100), entry("/media", 2048, 100)];
        let selected = select_root(&entries).expect("fallback volume");
        assert_eq!(selected.mount_point, "/media");
    }

    #[test]
    fn returns_none_without_volumes() {
        assert!(select_root(&[]).is_none());
    }

    #[test]
    fn computes_used_capacity_and_percentage() {
        let snap = snapshot(&entry("/", 512, 128));
        assert_eq!(snap.used_gb, 384);
        assert_eq!(snap.total_gb, 512);
        assert!((snap.used_pct - 75.0).abs() < 0.01);
    }

    #[test]
    fn reports_zero_percentage_for_an_empty_volume() {
        let snap = snapshot(&entry("/", 0, 0));
        assert_eq!(snap.total_gb, 0);
        assert!((snap.used_pct - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn clamps_used_capacity_when_available_exceeds_total() {
        let mut oversized = entry("/", 100, 0);
        oversized.available_bytes = 200 * BYTES_PER_GIB;
        let snap = snapshot(&oversized);
        assert_eq!(snap.used_gb, 0);
        assert!((snap.used_pct - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn renders_the_device_label_without_dev_prefix() {
        let snap = snapshot(&entry("/", 512, 128));
        assert_eq!(snap.label(), "nvme0n1p2 (ext4, SSD) on /");
    }

    #[test]
    fn strips_the_dev_prefix_from_raw_device_names() {
        assert_eq!(device_name(OsStr::new("/dev/nvme0n1p2")), "nvme0n1p2");
        assert_eq!(device_name(OsStr::new("overlay")), "overlay");
    }
}
