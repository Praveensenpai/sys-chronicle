use regex::Regex;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnimeInfo {
    pub title: String,
    pub episode: u32,
    pub season: Option<u32>,
}

pub struct AnimeParser;

impl AnimeParser {
    pub fn parse(raw_name: &str) -> Option<AnimeInfo> {
        let base_name = Self::extract_base_name(raw_name);
        let stripped = Self::strip_release_and_spec_tags(&base_name);
        let normalized = stripped.replace('_', " ");
        Self::extract_title_and_episode(&normalized)
    }

    fn extract_base_name(raw: &str) -> String {
        let p = Path::new(raw);
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| raw.to_string());

        let extensions = [".mkv", ".mp4", ".avi", ".webm", ".mov", ".m4v"];
        let mut clean = name.as_str();
        for ext in extensions {
            if clean.to_ascii_lowercase().ends_with(ext) {
                clean = &clean[..clean.len() - ext.len()];
                break;
            }
        }
        clean.to_string()
    }

    fn strip_release_and_spec_tags(name: &str) -> String {
        let Ok(re_group) = Regex::new(r"^\[[^\]]+\]\s*") else {
            return name.to_string();
        };
        let without_group = re_group.replace(name, "").to_string();

        let Ok(re_specs) = Regex::new(
            r"(?i)\[(?:[0-9a-f]{8}|1080p|720p|480p|hevc|x265|x264|10bit|dual-audio|flac|aac)[^\]]*\]|\((?:1080p|720p|480p|bd|web-dl|bluray)[^)]*\)",
        ) else {
            return without_group;
        };
        let cleaned = re_specs.replace_all(&without_group, "").to_string();
        cleaned.trim().to_string()
    }

    fn extract_title_and_episode(text: &str) -> Option<AnimeInfo> {
        // Pattern 1: S01E04 or E04
        if let Ok(re_se) = Regex::new(r"(?i)(.*?)\s+(?:S(\d{1,2}))?E(\d{1,4})(?:\s|$)") {
            if let Some(caps) = re_se.captures(text) {
                let title = caps.get(1)?.as_str().trim();
                let season = caps.get(2).and_then(|m| m.as_str().parse::<u32>().ok());
                let ep = caps.get(3)?.as_str().parse::<u32>().ok()?;
                return Self::build_info(title, ep, season);
            }
        }

        // Pattern 2: " - 04" or " - 04v2"
        if let Ok(re_dash) = Regex::new(r"(.*?)\s+-\s+(\d{1,4})(?:v\d+)?(?:\s|$)") {
            if let Some(caps) = re_dash.captures(text) {
                let title = caps.get(1)?.as_str().trim();
                let ep = caps.get(2)?.as_str().parse::<u32>().ok()?;
                return Self::build_info(title, ep, None);
            }
        }

        // Pattern 3: "Episode 04" or "Ep 4"
        if let Ok(re_ep) = Regex::new(r"(?i)(.*?)\s+(?:episode|ep\.?)\s*(\d{1,4})(?:\s|$)") {
            if let Some(caps) = re_ep.captures(text) {
                let title = caps.get(1)?.as_str().trim();
                let ep = caps.get(2)?.as_str().parse::<u32>().ok()?;
                return Self::build_info(title, ep, None);
            }
        }

        // Pattern 4: Trailing number "Title 04"
        if let Ok(re_num) = Regex::new(r"(.*?)\s+(\d{1,4})(?:v\d+)?$") {
            if let Some(caps) = re_num.captures(text) {
                let title = caps.get(1)?.as_str().trim();
                let ep = caps.get(2)?.as_str().parse::<u32>().ok()?;
                return Self::build_info(title, ep, None);
            }
        }

        None
    }

    fn extract_season_from_title(title: &str) -> Option<u32> {
        if let Ok(re_season) = Regex::new(r"(?i)\b(?:season|s)\s*(\d{1,2})\b") {
            if let Some(caps) = re_season.captures(title) {
                if let Some(num) = caps.get(1).and_then(|m| m.as_str().parse::<u32>().ok()) {
                    return Some(num);
                }
            }
        }
        if let Ok(re_ord) = Regex::new(r"(?i)\b(\d{1,2})(?:nd|rd|th|st)\s*season\b") {
            if let Some(caps) = re_ord.captures(title) {
                if let Some(num) = caps.get(1).and_then(|m| m.as_str().parse::<u32>().ok()) {
                    return Some(num);
                }
            }
        }
        None
    }

    fn build_info(
        raw_title: &str,
        episode: u32,
        explicit_season: Option<u32>,
    ) -> Option<AnimeInfo> {
        let clean_title = raw_title
            .replace('_', " ")
            .trim_matches(|c: char| c == '-' || c == ' ' || c == '.')
            .trim()
            .to_string();

        if clean_title.is_empty() || episode == 0 {
            None
        } else {
            let season = explicit_season.or_else(|| Self::extract_season_from_title(&clean_title));
            Some(AnimeInfo {
                title: clean_title,
                episode,
                season,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subsplease_format() {
        let raw = "[SubsPlease] Sousou no Frieren - 04 (1080p) [9A1B2C3D].mkv";
        let info = AnimeParser::parse(raw).expect("parsed");
        assert_eq!(info.title, "Sousou no Frieren");
        assert_eq!(info.episode, 4);
    }

    #[test]
    fn test_season_episode_format() {
        let raw = "Chainsaw Man S01E12 1080p.mkv";
        let info = AnimeParser::parse(raw).expect("parsed");
        assert_eq!(info.title, "Chainsaw Man");
        assert_eq!(info.episode, 12);
        assert_eq!(info.season, Some(1));
    }

    #[test]
    fn test_episode_prefix_format() {
        let raw = "One Piece Episode 1085.mp4";
        let info = AnimeParser::parse(raw).expect("parsed");
        assert_eq!(info.title, "One Piece");
        assert_eq!(info.episode, 1085);
        assert_eq!(info.season, None);
    }

    #[test]
    fn test_underscored_format() {
        let raw = "[Erai-raws] Jujutsu_Kaisen_2nd_Season_-_05_[1080p].mkv";
        let info = AnimeParser::parse(raw).expect("parsed");
        assert_eq!(info.title, "Jujutsu Kaisen 2nd Season");
        assert_eq!(info.episode, 5);
        assert_eq!(info.season, Some(2));
    }
}
