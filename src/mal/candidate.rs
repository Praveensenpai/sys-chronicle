use super::client::{MalAnimeNode, MediaKind};
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

/// Detect season number and media hints encoded in a title string.
///
/// Returns `(season, is_special, is_movie)`. The special/movie flags are a
/// last-resort fallback for nodes that carry no MAL `media_type`.
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

/// Strip filename noise that a canonical title can still carry.
///
/// `canonical_title` is not always a clean series name. When the AI title pass
/// misses, it keeps the literal filename, e.g.
/// `Laid-Back Camp - The Movie (2022) [Japanese] [1080p].mkv`. A query built
/// from that always returns zero MAL results, so drop the media extension and
/// any bracketed release/spec tags first.
fn strip_file_noise(title: &str) -> String {
    let mut text = title.to_string();

    let lower = text.to_ascii_lowercase();
    for ext in [".mkv", ".mp4", ".avi", ".webm", ".mov", ".m4v"] {
        if lower.ends_with(ext) {
            text.truncate(text.len() - ext.len());
            break;
        }
    }

    if let Ok(re_tags) = Regex::new(r"\[[^\]]*\]") {
        text = re_tags.replace_all(&text, " ").to_string();
    }

    text.replace(&['_', '.'][..], " ").trim().to_string()
}

/// Build the best starting MAL search string from a canonical title.
///
/// Canonical titles frequently keep a trailing release year (`... (2022)`) or
/// the word `Movie`. MAL matches queries literally, so either poisons the
/// search and yields zero results. Strip those, plus any article left dangling
/// at the end after the strip.
pub fn seed_query_from_title(title: &str) -> String {
    // Do NOT use `normalize_for_comparison` here: it deletes punctuation, so
    // `Laid-Back Camp` collapses to `laidback camp`, which MAL matches
    // literally and returns zero hits for. For outbound queries we keep word
    // boundaries by turning punctuation into spaces instead.
    let cleaned = strip_file_noise(title);
    let lower = cleaned.to_lowercase();
    let spaced: String = lower
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c.is_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect();
    let collapsed = spaced.split_whitespace().collect::<Vec<_>>().join(" ");

    // Drop episode/season markers and media-type words that poison MAL search.
    let mut sanitized = strip_season_and_type_tags(&collapsed);
    if let Ok(re_ep) = Regex::new(r"(?i)\bs\d{1,2}e\d{1,4}\b|\be\d{1,4}\b") {
        sanitized = re_ep.replace_all(&sanitized, " ").to_string();
    }
    let stripped = sanitized.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut words: Vec<&str> = stripped.split_whitespace().collect();

    if let Some(last) = words.last() {
        if last.len() == 4 && last.chars().all(|c| c.is_ascii_digit()) {
            words.pop();
        }
    }

    while let Some(last) = words.last() {
        if matches!(*last, "the" | "a" | "an" | "of" | "no") {
            words.pop();
        } else {
            break;
        }
    }

    words.join(" ")
}

/// Resolve the media kind of a node, preferring MAL's own `media_type` and</｜｜DSML｜｜>
/// only falling back to title keywords when MAL did not report one.
fn node_kind(node: &MalAnimeNode) -> MediaKind {
    let kind = node.media_kind();
    if kind != MediaKind::Unknown {
        return kind;
    }
    let (_, is_special, is_movie) = detect_season_and_type(&node.title);
    if is_movie {
        MediaKind::Movie
    } else if is_special {
        MediaKind::Special
    } else {
        MediaKind::Unknown
    }
}

/// Infer a season number from folder names such as `Season 03` or `S03`.
///
/// The AI parser often omits the season, while the on-disk layout still
/// encodes it, so the path is a reliable fallback before assuming season 1.
pub fn season_from_path(media_path: &str) -> Option<u32> {
    let path = std::path::Path::new(media_path);
    let mut found = None;
    for component in path.components() {
        let text = component.as_os_str().to_string_lossy().to_string();
        if let (Some(season), _, _) = detect_season_and_type(&text) {
            found = Some(season);
        }
    }
    found
}

/// Best available season number for a node, checking synonyms too.
fn node_season(node: &MalAnimeNode) -> Option<u32> {
    for title in node.all_titles() {
        if let (Some(season), _, _) = detect_season_and_type(title) {
            return Some(season);
        }
    }
    None
}

/// Best base-title similarity between the query and any known title of the node.
fn base_similarity(node: &MalAnimeNode, norm_query: &str) -> i32 {
    let mut best = i32::MIN;
    for title in node.all_titles() {
        let norm = normalize_for_comparison(title);
        let base = strip_season_and_type_tags(&norm);
        let similarity = if base == norm_query {
            150
        } else if base.starts_with(norm_query) || norm_query.starts_with(&base) {
            80
        } else if norm.contains(norm_query) {
            40
        } else {
            0
        };
        best = best.max(similarity);
    }
    if best == i32::MIN {
        0
    } else {
        best
    }
}

/// Score a candidate, or return `None` when it must be excluded outright.
///
/// A series query never accepts a special/movie entry, and a known target
/// season never accepts an entry from a different season.
pub fn score_candidate(
    candidate: &MalAnimeNode,
    query_title: &str,
    target_season: Option<u32>,
    query_kind: MediaKind,
) -> Option<i32> {
    let norm_query = strip_season_and_type_tags(&normalize_for_comparison(query_title));
    if norm_query.is_empty() {
        return None;
    }

    let kind = node_kind(candidate);
    let similarity = base_similarity(candidate, &norm_query);
    if similarity == 0 {
        return None;
    }

    let mut score = similarity;

    match query_kind {
        MediaKind::Special => {
            if kind == MediaKind::Special {
                score += 120;
            } else {
                return None;
            }
        }
        MediaKind::Movie => {
            if kind == MediaKind::Movie {
                score += 120;
            } else {
                return None;
            }
        }
        _ => {
            if kind == MediaKind::Special || kind == MediaKind::Movie {
                return None;
            }
            let effective_season = node_season(candidate).unwrap_or(1);
            match target_season {
                Some(target) if effective_season == target => score += 100,
                Some(_) => return None,
                None => {
                    if effective_season != 1 {
                        score -= 100;
                    }
                }
            }
            if let Some(ref status) = candidate.my_list_status {
                score += 50;
                if status.num_episodes_watched > 0 {
                    score += 20;
                }
            }
        }
    }

    let norm_cand = normalize_for_comparison(&candidate.title);
    let len_diff = (norm_cand.len() as i32 - norm_query.len() as i32).abs();
    score -= len_diff.min(30);

    Some(score)
}

/// Build the ordered list of MAL search queries to try for one media item.
///
/// MAL's relevance ranking drops sequels when only the base title is queried,
/// so a known season always adds a `"<title> Season N"` query. Duplicates are
/// removed while keeping the original order.
pub fn build_search_queries(title: &str, season: Option<u32>) -> Vec<String> {
    let mut queries = Vec::new();
    let base = title.trim();
    if base.is_empty() {
        return queries;
    }
    queries.push(base.to_string());

    let canonical = normalize_for_comparison(base);
    let stripped = strip_season_and_type_tags(&canonical);
    let title_is_tagged = !stripped.is_empty() && stripped != canonical;
    let (title_season, _, _) = detect_season_and_type(base);

    match season.filter(|s| *s > 1) {
        Some(target) if title_is_tagged && title_season == Some(target) => {
            // The title already names the exact season; no extra query needed.
        }
        Some(target) => {
            if title_is_tagged {
                queries.push(stripped.clone());
            }
            let stem = if title_is_tagged {
                stripped.as_str()
            } else {
                base
            };
            queries.push(format!("{} Season {}", stem, target));
        }
        None => {
            if title_is_tagged {
                queries.push(stripped.clone());
            }
        }
    }

    let mut seen = std::collections::HashSet::new();
    queries.retain(|q| seen.insert(q.to_lowercase()));
    queries
}

pub fn select_best_candidate<'a>(
    candidates: &'a [MalAnimeNode],
    query_title: &str,
    target_season: Option<u32>,
) -> Option<&'a MalAnimeNode> {
    let (query_season, query_special, query_movie) = detect_season_and_type(query_title);
    let query_kind = if query_movie {
        MediaKind::Movie
    } else if query_special {
        MediaKind::Special
    } else {
        MediaKind::Series
    };
    let resolved_season = match query_kind {
        MediaKind::Series => target_season.or(query_season),
        _ => None,
    };

    let mut scored: Vec<ScoredCandidate<'a>> = candidates
        .iter()
        .filter_map(|node| {
            score_candidate(node, query_title, resolved_season, query_kind)
                .map(|score| ScoredCandidate { node, score })
        })
        .collect();

    scored.sort_by_key(|b| std::cmp::Reverse(b.score));
    scored.first().map(|sc| sc.node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mal::client::{AlternativeTitles, UserListStatus};

    fn make_node(id: u64, title: &str, media_type: &str, in_list: bool) -> MalAnimeNode {
        MalAnimeNode {
            id,
            title: title.to_string(),
            num_episodes: 12,
            media_type: Some(media_type.to_string()),
            alternative_titles: None,
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
            make_node(38474, "Yuru Camp△ Season 2", "tv", false),
            make_node(34798, "Yuru Camp△", "tv", true),
            make_node(53410, "Yuru Camp△ Season 3", "tv", false),
            make_node(38475, "Yuru Camp△ Movie", "movie", false),
        ];

        let best = select_best_candidate(&candidates, "Yuru Camp", None);
        assert_eq!(best.unwrap().id, 34798);
    }

    #[test]
    fn test_selects_season_2_when_target_season_specified() {
        let candidates = vec![
            make_node(34798, "Yuru Camp△", "tv", true),
            make_node(38474, "Yuru Camp△ Season 2", "tv", false),
            make_node(53410, "Yuru Camp△ Season 3", "tv", false),
        ];

        let best = select_best_candidate(&candidates, "Yuru Camp", Some(2));
        assert_eq!(best.unwrap().id, 38474);
    }

    #[test]
    fn test_special_never_beats_real_season_3() {
        let mut real = make_node(53410, "Yuru Camp△ Season 3", "tv", false);
        real.alternative_titles = Some(AlternativeTitles {
            synonyms: vec!["Yuru Camp 3rd Season".into()],
            english: Some("Laid-Back Camp Season 3".into()),
            japanese: None,
        });
        let candidates = vec![
            make_node(58855, "Yuru Camp△ Season 3 Specials", "special", false),
            real,
        ];

        let best = select_best_candidate(&candidates, "Laid-Back Camp", Some(3));
        assert_eq!(best.unwrap().id, 53410);
    }

    #[test]
    fn test_special_only_candidate_for_season_returns_none() {
        let candidates = vec![make_node(
            58855,
            "Yuru Camp△ Season 3 Specials",
            "special",
            false,
        )];

        assert!(select_best_candidate(&candidates, "Laid-Back Camp", Some(3)).is_none());
    }

    #[test]
    fn test_movie_never_matches_series_query() {
        let candidates = vec![make_node(38475, "Yuru Camp△ Movie", "movie", false)];

        assert!(select_best_candidate(&candidates, "Yuru Camp", Some(3)).is_none());
    }

    #[test]
    fn test_query_builder_adds_season_and_dedupes() {
        let queries = build_search_queries("Laid-Back Camp", Some(3));
        assert_eq!(
            queries,
            vec![
                "Laid-Back Camp".to_string(),
                "Laid-Back Camp Season 3".to_string()
            ]
        );

        let already = build_search_queries("Yuru Camp Season 3", Some(3));
        assert_eq!(already, vec!["Yuru Camp Season 3".to_string()]);

        assert!(build_search_queries("   ", Some(3)).is_empty());
    }

    #[test]
    fn test_synonym_matches_base_title() {
        let mut node = make_node(53410, "Yuru Camp△ Season 3", "tv", false);
        node.alternative_titles = Some(AlternativeTitles {
            synonyms: vec!["Laid-Back Camp 3rd Season".into()],
            english: None,
            japanese: None,
        });
        let candidates = vec![node];

        let best = select_best_candidate(&candidates, "Laid-Back Camp", Some(3));
        assert_eq!(best.unwrap().id, 53410);
    }

    #[test]
    fn seed_query_drops_year_and_dangling_article() {
        assert_eq!(
            seed_query_from_title("Laid-Back Camp the Movie (2022)"),
            "laid back camp"
        );
        assert_eq!(seed_query_from_title("Yuru Camp△ Season 2"), "yuru camp");
        assert_eq!(seed_query_from_title(""), "");
    }

    #[test]
    fn seed_query_strips_filename_and_release_tags() {
        // Real stored record: the AI title pass kept the whole filename.
        // MAL matches literally, so hyphens must survive as separators.
        assert_eq!(
            seed_query_from_title("Laid-Back Camp - The Movie (2022) [Japanese] [1080p].mkv"),
            "laid back camp"
        );
        assert_eq!(
            seed_query_from_title("Laid-Back Camp - S03E01 [Japanese] [1080p].mkv"),
            "laid back camp"
        );
        assert_eq!(seed_query_from_title("Yuru Camp△ Season 2"), "yuru camp");
    }
}
