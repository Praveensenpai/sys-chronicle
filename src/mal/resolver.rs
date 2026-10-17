use anyhow::Result;

use super::candidate::{build_search_queries, season_from_path, select_best_candidate};
use super::client::{MalAnimeNode, MalClient};
use super::overrides::AnimeOverrides;
use super::parser::AnimeInfo;

/// Result of resolving a media file to a MAL entry.
#[derive(Debug, Clone)]
pub struct ResolvedAnime {
    pub anime: MalAnimeNode,
    /// True when the entry came from a manual folder override.
    pub from_override: bool,
}

/// Resolve a media file to the MAL entry it should be scrobbled against.
///
/// A manual folder override always wins over search. Otherwise every query
/// produced by [`build_search_queries`] is tried, results are merged (deduped
/// by MAL id), and the best candidate is selected once. Searching only the
/// base title is what previously lost sequel entries such as "Season 3".
pub async fn resolve_anime(
    client: &MalClient,
    media_path: &str,
    info: &AnimeInfo,
) -> Result<Option<ResolvedAnime>> {
    if let Some(resolved) = resolve_from_override(client, media_path).await? {
        return Ok(Some(resolved));
    }

    // The AI often drops the season; recover it from the folder layout
    // ("Season 03", "S03") so the season-specific query is still issued.
    let season = info.season.or_else(|| season_from_path(media_path));
    let queries = build_search_queries(&info.title, season);
    let mut merged: Vec<MalAnimeNode> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for query in queries {
        match client.search_anime(&query).await {
            Ok(nodes) => {
                for node in nodes {
                    if seen.insert(node.id) {
                        merged.push(node);
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "[sys-chronicle-mal] MAL search failed for query \"{}\": {}",
                    query, e
                );
            }
        }
    }

    if merged.is_empty() {
        return Ok(None);
    }

    let Some(anime) = select_best_candidate(&merged, &info.title, info.season).cloned() else {
        return Ok(None);
    };
    Ok(Some(ResolvedAnime {
        anime,
        from_override: false,
    }))
}

/// Look up a manual folder override for the given media path.
pub async fn resolve_from_override(
    client: &MalClient,
    media_path: &str,
) -> Result<Option<ResolvedAnime>> {
    let Ok(overrides) = AnimeOverrides::load() else {
        return Ok(None);
    };
    let Some(entry) = overrides.lookup(media_path).cloned() else {
        return Ok(None);
    };

    let file_name = std::path::Path::new(media_path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    let target_id = entry.id_for(&file_name);

    let mut anime = client.get_anime(target_id).await?;
    if anime.num_episodes == 0 {
        if let Some(total) = entry.total_episodes {
            anime.num_episodes = total;
        }
    }
    println!(
        "[sys-chronicle-mal] Manual folder override: \"{}\" -> {} ({})",
        entry.folder, anime.title, anime.id
    );
    Ok(Some(ResolvedAnime {
        anime,
        from_override: true,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_cover_base_and_season() {
        let info = AnimeInfo {
            title: "Laid-Back Camp".into(),
            episode: 3,
            season: Some(3),
        };
        let queries = build_search_queries(&info.title, info.season);
        assert!(queries.iter().any(|q| q.contains("Season 3")));
    }

    #[test]
    fn season_is_recovered_from_folder_name() {
        let path = "/media/Anime/Season 03/Laid-Back Camp - S03E03 [1080p].mkv";
        assert_eq!(season_from_path(path), Some(3));

        let nested = "/media/Anime/Yuru Camp/Season 02/ep.mkv";
        assert_eq!(season_from_path(nested), Some(2));

        let flat = "/media/Anime/Yuru Camp/ep.mkv";
        assert_eq!(season_from_path(flat), None);
    }
}
