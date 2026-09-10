use super::client::MalAnimeNode;
use regex::Regex;

#[derive(Debug, Clone)]
pub struct ScoredCandidate<'a> {
    pub node: &'a MalAnimeNode,
    pub score: i32,
}

pub fn normalize_for_comparison(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn detect_season_and_type(title: &str) -> (Option<u32>, bool, bool) {
    let lower = title.to_lowercase();
    let is_movie =
        lower.contains("movie") || lower.contains("film") || lower.contains("gekijooban");
    let is_special = lower.contains("special")
        || lower.contains("specials")
        || lower.contains(" ova")
        || lower.contains(" oad")
        || lower.starts_with("ova")
        || lower.starts_with("sp ");

    if let Ok(re_season) = Regex::new(r"(?i)\b(?:season|s)\s*(\d{1,2})\b") {
        if let Some(caps) = re_season.captures(title) {
            if let Some(num) = caps.get(1).and_then(|m| m.as_str().parse::<u32>().ok()) {
                return (Some(num), is_special, is_movie);
            }
        }
    }

    if let Ok(re_ord) = Regex::new(r"(?i)\b(\d{1,2})(?:nd|rd|th|st)\s*season\b") {
        if let Some(caps) = re_ord.captures(title) {
            if let Some(num) = caps.get(1).and_then(|m| m.as_str().parse::<u32>().ok()) {
                return (Some(num), is_special, is_movie);
            }
        }
    }

    (None, is_special, is_movie)
}

pub fn strip_season_and_type_tags(normalized: &str) -> String {
    if let Ok(re) =
        Regex::new(r"(?i)\b(season\s*\d+|\d+(?:nd|rd|th|st)\s*season|movie|specials?|ova|oad)\b")
    {
        let stripped = re.replace_all(normalized, "").to_string();
        return stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    normalized.to_string()
}

pub fn score_candidate(
    candidate: &MalAnimeNode,
    query_title: &str,
    target_season: u32,
    query_is_special: bool,
    query_is_movie: bool,
) -> i32 {
    let norm_query = normalize_for_comparison(query_title);
    let norm_cand = normalize_for_comparison(&candidate.title);
    let base_cand = strip_season_and_type_tags(&norm_cand);
    let (cand_season, cand_special, cand_movie) = detect_season_and_type(&candidate.title);
    let effective_cand_season = cand_season.unwrap_or(1);

    let mut score: i32 = 0;

    if base_cand == norm_query {
        score += 150;
    } else if base_cand.starts_with(&norm_query) || norm_query.starts_with(&base_cand) {
        score += 80;
    } else if norm_cand.contains(&norm_query) {
        score += 40;
    }

    if query_is_movie {
        if cand_movie {
            score += 120;
        } else {
            score -= 120;
        }
    } else if query_is_special {
        if cand_special {
            score += 120;
        } else {
            score -= 120;
        }
    } else {
        if cand_movie {
            score -= 200;
        }
        if cand_special {
            score -= 200;
        }
        if effective_cand_season == target_season {
            score += 100;
        } else {
            score -= 200;
        }
    }

    if let Some(ref status) = candidate.my_list_status {
        if effective_cand_season == target_season {
            score += 50;
            if status.num_episodes_watched > 0 {
                score += 20;
            }
        }
    }

    let len_diff = (norm_cand.len() as i32 - norm_query.len() as i32).abs();
    score -= len_diff.min(30);

    score
}

pub fn select_best_candidate<'a>(
    candidates: &'a [MalAnimeNode],
    query_title: &str,
    target_season: Option<u32>,
) -> Option<&'a MalAnimeNode> {
    if candidates.is_empty() {
        return None;
    }

    let (query_season, query_special, query_movie) = detect_season_and_type(query_title);
    let resolved_season = target_season.or(query_season).unwrap_or(1);

    let mut scored: Vec<ScoredCandidate<'a>> = candidates
        .iter()
        .map(|node| ScoredCandidate {
            node,
            score: score_candidate(
                node,
                query_title,
                resolved_season,
                query_special,
                query_movie,
            ),
        })
        .collect();

    scored.sort_by_key(|b| std::cmp::Reverse(b.score));

    scored.first().map(|sc| sc.node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mal::client::UserListStatus;

    fn make_node(id: u64, title: &str, in_list: bool) -> MalAnimeNode {
        MalAnimeNode {
            id,
            title: title.to_string(),
            num_episodes: 12,
            my_list_status: if in_list {
                Some(UserListStatus {
                    status: Some("watching".into()),
                    num_episodes_watched: 0,
                })
            } else {
                None
            },
        }
    }

    #[test]
    fn test_selects_season_1_over_season_2_when_no_season_specified() {
        let candidates = vec![
            make_node(38474, "Yuru Camp△ Season 2", false),
            make_node(34798, "Yuru Camp△", true),
            make_node(53410, "Yuru Camp△ Season 3", false),
            make_node(38475, "Yuru Camp△ Movie", false),
        ];

        let best = select_best_candidate(&candidates, "Yuru Camp", None);
        assert_eq!(best.unwrap().id, 34798);
        assert_eq!(best.unwrap().title, "Yuru Camp△");
    }

    #[test]
    fn test_selects_season_2_when_target_season_specified() {
        let candidates = vec![
            make_node(34798, "Yuru Camp△", true),
            make_node(38474, "Yuru Camp△ Season 2", false),
            make_node(53410, "Yuru Camp△ Season 3", false),
        ];

        let best = select_best_candidate(&candidates, "Yuru Camp", Some(2));
        assert_eq!(best.unwrap().id, 38474);
        assert_eq!(best.unwrap().title, "Yuru Camp△ Season 2");
    }

    #[test]
    fn test_penalizes_specials_and_movies() {
        let candidates = vec![
            make_node(38475, "Yuru Camp△ Movie", false),
            make_node(37341, "Yuru Camp△ Specials", false),
            make_node(34798, "Yuru Camp△", false),
        ];

        let best = select_best_candidate(&candidates, "Yuru Camp", None);
        assert_eq!(best.unwrap().id, 34798);
    }
}
