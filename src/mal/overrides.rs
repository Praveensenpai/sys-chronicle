use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::history::{AnimeSyncHistory, AnimeSyncRecord};

/// A manual, folder-scoped MAL binding.
///
/// Every media file located inside `folder` (including specials and files with
/// slightly different names) resolves to `mal_anime_id` instead of whatever
/// the automatic search would have picked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimeOverride {
    /// Absolute, canonical parent folder this binding applies to.
    pub folder: String,
    /// MAL id used for regular episodes.
    pub mal_anime_id: u64,
    /// Canonical title shown in the TUI for this folder.
    pub canonical_title: String,
    #[serde(default)]
    pub total_episodes: Option<u32>,
    /// Optional MAL id used for files named as specials in this folder.
    #[serde(default)]
    pub specials_mal_anime_id: Option<u64>,
    #[serde(default)]
    pub updated_at: String,
}

impl AnimeOverride {
    /// Id that should be used for a file inside this folder.
    pub fn id_for(&self, file_name: &str) -> u64 {
        let lower = file_name.to_lowercase();
        let is_special = lower.contains("special")
            || lower.contains(" ova")
            || lower.contains(" oad")
            || lower.contains("other")
            || lower.contains("[sp");
        match (is_special, self.specials_mal_anime_id) {
            (true, Some(id)) => id,
            _ => self.mal_anime_id,
        }
    }
}

pub struct AnimeOverrides {
    entries: Vec<AnimeOverride>,
}

impl AnimeOverrides {
    pub fn storage_path() -> PathBuf {
        #[cfg(test)]
        return std::env::temp_dir().join("sys_chronicle_test_anime_overrides.json");
        #[cfg(not(test))]
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("sys-chronicle")
            .join("anime_overrides.json")
    }

    pub fn load() -> Result<Self> {
        let path = Self::storage_path();
        if !path.exists() {
            return Ok(Self {
                entries: Vec::new(),
            });
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read overrides from {:?}", path))?;
        if content.trim().is_empty() {
            return Ok(Self {
                entries: Vec::new(),
            });
        }
        let entries: Vec<AnimeOverride> = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse overrides from {:?}", path))?;
        Ok(Self { entries })
    }

    pub fn entries(&self) -> &[AnimeOverride] {
        &self.entries
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::storage_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(&self.entries)?;
        let tmp_path = path.with_extension(format!("tmp.{}", std::process::id()));
        {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp_path)?;
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp_path, &path)?;
        Ok(())
    }

    /// Insert or replace the binding for a folder. Returns the stored entry.
    pub fn set(
        &mut self,
        folder: &Path,
        mal_anime_id: u64,
        canonical_title: &str,
        total_episodes: Option<u32>,
    ) -> AnimeOverride {
        let folder = canonical_folder(folder);
        let entry = AnimeOverride {
            folder,
            mal_anime_id,
            canonical_title: canonical_title.to_string(),
            total_episodes,
            specials_mal_anime_id: None,
            updated_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        };
        self.entries.retain(|e| e.folder != entry.folder);
        self.entries.push(entry.clone());
        entry
    }

    /// Remove the binding for a folder, returning the removed entry.
    pub fn remove(&mut self, folder: &Path) -> Option<AnimeOverride> {
        let folder = canonical_folder(folder);
        let idx = self.entries.iter().position(|e| e.folder == folder)?;
        Some(self.entries.remove(idx))
    }

    /// Longest-ancestor-prefix lookup for a media file path.
    pub fn lookup(&self, media_path: &str) -> Option<&AnimeOverride> {
        let media = Path::new(media_path);
        let parent = media.parent().unwrap_or(media);
        let parent = canonical_folder(parent);

        let mut best: Option<(&AnimeOverride, usize)> = None;
        for entry in &self.entries {
            if path_within(&parent, &entry.folder) {
                let depth = entry.folder.len();
                if best.map(|(_, d)| depth > d).unwrap_or(true) {
                    best = Some((entry, depth));
                }
            }
        }
        best.map(|(entry, _)| entry)
    }
}

/// The folder a media file lives in, canonicalized when the path exists.
fn canonical_folder(folder: &Path) -> String {
    std::fs::canonicalize(folder)
        .unwrap_or_else(|_| folder.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn path_within(child: &str, ancestor: &str) -> bool {
    if child == ancestor {
        return true;
    }
    let prefix = format!(
        "{}{}",
        ancestor.trim_end_matches('/'),
        std::path::MAIN_SEPARATOR
    );
    child.starts_with(&prefix)
}

/// Rewrite already-recorded history rows for `folder` so the corrected MAL
/// binding takes effect immediately, without waiting for a re-sync.
/// Returns the number of records touched.
pub fn reassign_history(
    folder: &Path,
    mal_anime_id: u64,
    canonical_title: &str,
    total_episodes: Option<u32>,
) -> Result<usize> {
    let folder = canonical_folder(folder);
    let mut records = AnimeSyncHistory::load_all().unwrap_or_default();
    let mut touched = 0;

    for record in &mut records {
        let Some(ref path) = record.path else {
            continue;
        };
        let parent = Path::new(path)
            .parent()
            .map(canonical_folder)
            .unwrap_or_default();
        if !path_within(&parent, &folder) {
            continue;
        }
        if touches_record(record, mal_anime_id, canonical_title, total_episodes) {
            touched += 1;
        }
    }

    if touched > 0 {
        AnimeSyncHistory::save_all(&records)?;
    }
    Ok(touched)
}

fn touches_record(
    record: &mut AnimeSyncRecord,
    mal_anime_id: u64,
    canonical_title: &str,
    total_episodes: Option<u32>,
) -> bool {
    let same = record.mal_anime_id == Some(mal_anime_id)
        && record.canonical_title == canonical_title
        && record.mal_total_episodes == total_episodes;
    if same {
        return false;
    }
    record.mal_anime_id = Some(mal_anime_id);
    record.canonical_title = canonical_title.to_string();
    record.mal_total_episodes = total_episodes;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(folder: &str, id: u64) -> AnimeOverride {
        AnimeOverride {
            folder: folder.to_string(),
            mal_anime_id: id,
            canonical_title: format!("Show {}", id),
            total_episodes: Some(12),
            specials_mal_anime_id: None,
            updated_at: String::new(),
        }
    }

    #[test]
    fn lookup_matches_parent_folder_for_any_filename() {
        let overrides = AnimeOverrides {
            entries: vec![entry("/media/Anime/Season 03", 53410)],
        };

        let hit = overrides.lookup("/media/Anime/Season 03/Laid-Back Camp - S03E05 [Japanese].mkv");
        assert_eq!(hit.map(|e| e.mal_anime_id), Some(53410));

        let special = overrides.lookup("/media/Anime/Season 03/Special - NCOP.mkv");
        assert_eq!(special.map(|e| e.mal_anime_id), Some(53410));

        let renamed = overrides.lookup("/media/Anime/Season 03/whatever renamed file.mkv");
        assert_eq!(renamed.map(|e| e.mal_anime_id), Some(53410));
    }

    #[test]
    fn lookup_prefers_longest_ancestor() {
        let overrides = AnimeOverrides {
            entries: vec![
                entry("/media/Anime", 1),
                entry("/media/Anime/Season 03", 53410),
            ],
        };

        let hit = overrides.lookup("/media/Anime/Season 03/ep.mkv");
        assert_eq!(hit.map(|e| e.mal_anime_id), Some(53410));
    }

    #[test]
    fn lookup_ignores_unrelated_folders() {
        let overrides = AnimeOverrides {
            entries: vec![entry("/media/Anime/Season 03", 53410)],
        };
        assert!(overrides
            .lookup("/media/Anime/Season 03 Extra/ep.mkv")
            .is_none());
    }

    #[test]
    fn specials_use_dedicated_id_when_configured() {
        let mut e = entry("/media/Anime/Season 03", 53410);
        e.specials_mal_anime_id = Some(58855);
        assert_eq!(e.id_for("Show - Special 01.mkv"), 58855);
        assert_eq!(e.id_for("Show - S03E01.mkv"), 53410);
    }
}
