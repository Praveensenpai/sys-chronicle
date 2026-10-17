use anyhow::{Context, Result};
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};

const MAL_API_BASE: &str = "https://api.myanimelist.net/v2";

#[derive(Debug, Clone, Deserialize)]
pub struct MalAnimeNode {
    pub id: u64,
    pub title: String,
    #[serde(default)]
    pub num_episodes: u32,
    #[serde(default)]
    pub media_type: Option<String>,
    #[serde(default)]
    pub alternative_titles: Option<AlternativeTitles>,
    pub my_list_status: Option<UserListStatus>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AlternativeTitles {
    #[serde(default)]
    pub synonyms: Vec<String>,
    #[serde(default, rename = "en")]
    pub english: Option<String>,
    #[serde(default, rename = "ja")]
    pub japanese: Option<String>,
}

impl MalAnimeNode {
    /// All titles this entry is known by (primary, synonyms, english).
    pub fn all_titles(&self) -> Vec<&str> {
        let mut titles = vec![self.title.as_str()];
        if let Some(ref alt) = self.alternative_titles {
            titles.extend(alt.synonyms.iter().map(String::as_str));
            if let Some(ref en) = alt.english {
                titles.push(en.as_str());
            }
        }
        titles
    }

    /// MAL `media_type` mapped onto the candidate-scoring media kind.
    pub fn media_kind(&self) -> MediaKind {
        match self.media_type.as_deref() {
            Some("movie") => MediaKind::Movie,
            Some("special") | Some("ova") | Some("ona") => MediaKind::Special,
            Some("tv") | Some("tv_special") => MediaKind::Series,
            _ => MediaKind::Unknown,
        }
    }
}

/// Coarse media category used when matching search hits against a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Series,
    Special,
    Movie,
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MalAnimeData {
    pub node: MalAnimeNode,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MalSearchResponse {
    pub data: Vec<MalAnimeData>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserListStatus {
    pub status: Option<String>,
    pub num_episodes_watched: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserProfile {
    pub id: u64,
    pub name: String,
}

pub struct MalClient {
    client: Client,
    token: String,
}

impl MalClient {
    pub fn new(token: String) -> Result<Self> {
        let mut headers = header::HeaderMap::new();
        let auth_value = format!("Bearer {}", token);
        let mut header_val = header::HeaderValue::from_str(&auth_value)
            .context("Invalid token format for auth header")?;
        header_val.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, header_val);

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .context("Failed to initialize HTTP client")?;

        Ok(Self { client, token })
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub async fn get_current_user(&self) -> Result<UserProfile> {
        let url = format!("{}/users/@me", MAL_API_BASE);
        let res = self.client.get(&url).send().await?;
        if !res.status().is_success() {
            let err = res.text().await.unwrap_or_default();
            anyhow::bail!("Failed to get user profile: {}", err);
        }
        res.json::<UserProfile>()
            .await
            .context("Failed to parse user profile response")
    }

    pub async fn search_anime(&self, query: &str) -> Result<Vec<MalAnimeNode>> {
        self.search_anime_limited(query, 10).await
    }

    pub async fn search_anime_limited(&self, query: &str, limit: u32) -> Result<Vec<MalAnimeNode>> {
        let url = format!("{}/anime", MAL_API_BASE);
        let limit_str = limit.to_string();
        let res = self
            .client
            .get(&url)
            .query(&[
                ("q", query),
                ("limit", limit_str.as_str()),
                (
                    "fields",
                    "id,title,num_episodes,media_type,alternative_titles,my_list_status",
                ),
            ])
            .send()
            .await?;

        if !res.status().is_success() {
            let err = res.text().await.unwrap_or_default();
            anyhow::bail!("Failed to search anime for query '{}': {}", query, err);
        }

        let search_res: MalSearchResponse = res
            .json()
            .await
            .context("Failed to parse search response")?;
        Ok(search_res.data.into_iter().map(|d| d.node).collect())
    }

    /// Fetch a single anime node by MAL id (used by manual folder overrides).
    pub async fn get_anime(&self, anime_id: u64) -> Result<MalAnimeNode> {
        let url = format!("{}/anime/{}", MAL_API_BASE, anime_id);
        let res = self
            .client
            .get(&url)
            .query(&[(
                "fields",
                "id,title,num_episodes,media_type,alternative_titles,my_list_status",
            )])
            .send()
            .await?;

        if !res.status().is_success() {
            let err = res.text().await.unwrap_or_default();
            anyhow::bail!("Failed to fetch anime {}: {}", anime_id, err);
        }

        res.json::<MalAnimeNode>()
            .await
            .with_context(|| format!("Failed to parse anime node {}", anime_id))
    }

    pub async fn update_episode_progress(
        &self,
        anime_id: u64,
        episode: u32,
        total_episodes: u32,
    ) -> Result<UserListStatus> {
        let url = format!("{}/anime/{}/my_list_status", MAL_API_BASE, anime_id);
        let status = if total_episodes > 0 && episode >= total_episodes {
            "completed"
        } else {
            "watching"
        };

        let ep_str = episode.to_string();
        let params = [("status", status), ("num_watched_episodes", &ep_str)];

        let res = self.client.put(&url).form(&params).send().await?;
        if !res.status().is_success() {
            let err = res.text().await.unwrap_or_default();
            anyhow::bail!("Failed to update anime progress on MAL: {}", err);
        }

        res.json::<UserListStatus>()
            .await
            .context("Failed to parse update response")
    }
}
