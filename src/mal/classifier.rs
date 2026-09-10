use anyhow::Result;
use regex::Regex;
use std::path::Path;

use super::gemini::GeminiParser;
use super::parser::AnimeInfo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaClassification {
    AnimeEpisode(AnimeInfo),
    NonSyncable {
        title: String,
        content_type: String,
        reason: String,
    },
}

pub struct AnimeClassifier;

impl AnimeClassifier {
    pub fn check_local_heuristics(raw_path_or_title: &str) -> Option<MediaClassification> {
        let p = Path::new(raw_path_or_title);
        let path_lower = raw_path_or_title.to_lowercase();

        for component in p.components() {
            let comp_str = component.as_os_str().to_string_lossy().to_lowercase();
            if comp_str == "extra"
                || comp_str == "extras"
                || comp_str == "bonus"
                || comp_str == "bonuses"
            {
                let content_type = if path_lower.contains("camping")
                    || path_lower.contains("live")
                    || path_lower.contains("interview")
                    || path_lower.contains("event")
                {
                    "live_action"
                } else if path_lower.contains("nced")
                    || path_lower.contains("ncop")
                    || path_lower.contains("creditless")
                {
                    "creditless"
                } else if path_lower.contains("sp") || path_lower.contains("special") {
                    "special"
                } else {
                    "bonus"
                };

                let file_stem = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| raw_path_or_title.to_string());

                return Some(MediaClassification::NonSyncable {
                    title: file_stem,
                    content_type: content_type.to_string(),
                    reason: "File located in bonus/extra directory".to_string(),
                });
            }
        }

        let filename = p
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| raw_path_or_title.to_string());
        let name_lower = filename.to_lowercase();

        if name_lower.contains("nced")
            || name_lower.contains("ncop")
            || name_lower.contains("creditless")
        {
            return Some(MediaClassification::NonSyncable {
                title: filename,
                content_type: "creditless".to_string(),
                reason: "Creditless opening / ending animation".to_string(),
            });
        }

        if name_lower.contains("menu.mkv")
            || name_lower.contains("menu.mp4")
            || name_lower.starts_with("menu ")
        {
            return Some(MediaClassification::NonSyncable {
                title: filename,
                content_type: "bonus".to_string(),
                reason: "Disc navigation menu".to_string(),
            });
        }

        if name_lower.contains("trailer")
            || name_lower.contains("preview")
            || name_lower.contains("teaser")
        {
            return Some(MediaClassification::NonSyncable {
                title: filename,
                content_type: "bonus".to_string(),
                reason: "Promotional trailer or preview".to_string(),
            });
        }

        if let Ok(re_sp) = Regex::new(r"(?i)(?:\[sp\d+\]|\bsp\d+\b)") {
            if re_sp.is_match(&name_lower) {
                let ct = if name_lower.contains("camping")
                    || name_lower.contains("live")
                    || name_lower.contains("interview")
                {
                    "live_action"
                } else {
                    "special"
                };
                return Some(MediaClassification::NonSyncable {
                    title: filename,
                    content_type: ct.to_string(),
                    reason: "Identified as a special/bonus feature clip".to_string(),
                });
            }
        }

        None
    }

    pub async fn classify(
        raw_path_or_title: &str,
        gemini: Option<&GeminiParser>,
    ) -> Result<Option<MediaClassification>> {
        if let Some(res) = Self::check_local_heuristics(raw_path_or_title) {
            return Ok(Some(res));
        }

        if let Some(parser) = gemini {
            if let Ok(Some(ai_result)) = parser.classify(raw_path_or_title).await {
                if !ai_result.is_syncable_anime {
                    let title = ai_result
                        .title
                        .unwrap_or_else(|| raw_path_or_title.to_string());
                    let content_type = ai_result
                        .content_type
                        .unwrap_or_else(|| "special".to_string());
                    let reason = ai_result
                        .reason
                        .unwrap_or_else(|| "AI classified as non-syncable media".to_string());
                    return Ok(Some(MediaClassification::NonSyncable {
                        title,
                        content_type,
                        reason,
                    }));
                } else if let (Some(title), Some(episode)) = (ai_result.title, ai_result.episode) {
                    return Ok(Some(MediaClassification::AnimeEpisode(AnimeInfo {
                        title,
                        episode,
                        season: ai_result.season,
                    })));
                }
            }
        }

        if let Some(info) = super::parser::AnimeParser::parse(raw_path_or_title) {
            return Ok(Some(MediaClassification::AnimeEpisode(info)));
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extra_directory_detected_as_non_syncable() {
        let path = "/home/user/Anime/Yuru Camp/EXTRA/Yuru Camp [SP05] Hanamori Yumiri First Time Camping - 01.mkv";
        let res = AnimeClassifier::check_local_heuristics(path);
        assert!(res.is_some());
        match res.unwrap() {
            MediaClassification::NonSyncable { content_type, .. } => {
                assert_eq!(content_type, "live_action");
            }
            _ => panic!("Expected NonSyncable"),
        }
    }

    #[test]
    fn test_creditless_detected_as_non_syncable() {
        let path = "[SubsPlease] Frieren - NCOP1 (1080p).mkv";
        let res = AnimeClassifier::check_local_heuristics(path);
        assert!(res.is_some());
        match res.unwrap() {
            MediaClassification::NonSyncable { content_type, .. } => {
                assert_eq!(content_type, "creditless");
            }
            _ => panic!("Expected NonSyncable"),
        }
    }

    #[test]
    fn test_standard_anime_not_flagged_by_heuristics() {
        let path = "/home/user/Anime/Yuru Camp/Yuru Camp - 01.mkv";
        let res = AnimeClassifier::check_local_heuristics(path);
        assert!(res.is_none());
    }
}
