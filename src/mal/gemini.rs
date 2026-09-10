use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::parser::AnimeInfo;

#[derive(Serialize)]
struct GeminiPart<'a> {
    text: &'a str,
}

#[derive(Serialize)]
struct GeminiContent<'a> {
    parts: Vec<GeminiPart<'a>>,
}

#[derive(Serialize)]
struct GeminiSchemaField {
    #[serde(rename = "type")]
    field_type: &'static str,
}

#[derive(Serialize)]
struct GeminiSchemaProps {
    title: GeminiSchemaField,
    episode: GeminiSchemaField,
}

#[derive(Serialize)]
struct GeminiResponseSchema {
    #[serde(rename = "type")]
    schema_type: &'static str,
    properties: GeminiSchemaProps,
    required: Vec<&'static str>,
}

#[derive(Serialize)]
struct GeminiGenConfig {
    #[serde(rename = "responseMimeType")]
    response_mime_type: &'static str,
    #[serde(rename = "responseSchema")]
    response_schema: GeminiResponseSchema,
}

#[derive(Serialize)]
struct GeminiRequest<'a> {
    contents: Vec<GeminiContent<'a>>,
    #[serde(rename = "generationConfig")]
    generation_config: GeminiGenConfig,
}

#[derive(Deserialize)]
struct GeminiResponsePart {
    text: Option<String>,
}

#[derive(Deserialize)]
struct GeminiResponseContent {
    parts: Option<Vec<GeminiResponsePart>>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiResponseContent>,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GeminiClassification {
    pub is_syncable_anime: bool,
    pub title: Option<String>,
    pub episode: Option<u32>,
    pub season: Option<u32>,
    pub content_type: Option<String>,
    pub reason: Option<String>,
}

#[derive(Deserialize)]
struct GeminiParsedAnime {
    title: String,
    episode: u32,
}

pub struct GeminiParser {
    api_key: String,
    model: String,
    client: Client,
}

impl GeminiParser {
    pub fn new(api_key: String, model_override: Option<String>) -> Result<Self> {
        let model = model_override
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| "gemini-3.5-flash".to_string());

        let client = Client::builder()
            .timeout(Duration::from_secs(6))
            .build()
            .context("Failed to build HTTP client for Gemini")?;

        Ok(Self {
            api_key,
            model,
            client,
        })
    }

    pub async fn classify(&self, raw_input: &str) -> Result<Option<GeminiClassification>> {
        let prompt = format!(
            "Classify this video file: \"{}\". Determine whether it is an actual animated anime episode of a TV series/OVA/Movie that should be synced to MyAnimeList, or if it is a bonus extra, special, creditless OP/ED, live-action footage, or non-anime content. Return JSON with keys: is_syncable_anime (bool), title (string or null), episode (int or null), season (int or null), content_type (string), reason (string).",
            raw_input
        );

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        let body = serde_json::json!({
            "contents": [{
                "parts": [{ "text": prompt }]
            }],
            "generationConfig": {
                "responseMimeType": "application/json"
            }
        });

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Gemini API request failed for model '{}'", self.model))?;

        if !response.status().is_success() {
            let status = response.status();
            let err_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Gemini API returned status {}: {}", status, err_text);
        }

        let gemini_resp: GeminiResponse = response
            .json()
            .await
            .context("Failed to deserialize Gemini API response")?;

        let Some(first_candidate) = gemini_resp.candidates.and_then(|c| c.into_iter().next())
        else {
            return Ok(None);
        };
        let Some(first_part) = first_candidate
            .content
            .and_then(|c| c.parts)
            .and_then(|p| p.into_iter().next())
        else {
            return Ok(None);
        };
        let Some(raw_json) = first_part.text else {
            return Ok(None);
        };

        let parsed: GeminiClassification = serde_json::from_str(&raw_json).with_context(|| {
            format!(
                "Failed to parse classification JSON from Gemini: {}",
                raw_json
            )
        })?;

        Ok(Some(parsed))
    }

    pub async fn parse(&self, raw_input: &str) -> Result<Option<AnimeInfo>> {
        let prompt = format!(
            "Extract the official MyAnimeList canonical anime title (English or Romaji) and episode number from this filename or title: \"{}\". Return JSON matching the schema.",
            raw_input
        );

        let req_body = GeminiRequest {
            contents: vec![GeminiContent {
                parts: vec![GeminiPart { text: &prompt }],
            }],
            generation_config: GeminiGenConfig {
                response_mime_type: "application/json",
                response_schema: GeminiResponseSchema {
                    schema_type: "OBJECT",
                    properties: GeminiSchemaProps {
                        title: GeminiSchemaField {
                            field_type: "STRING",
                        },
                        episode: GeminiSchemaField {
                            field_type: "INTEGER",
                        },
                    },
                    required: vec!["title", "episode"],
                },
            },
        };

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        let response = self
            .client
            .post(&url)
            .json(&req_body)
            .send()
            .await
            .with_context(|| format!("Gemini API request failed for model '{}'", self.model))?;

        if !response.status().is_success() {
            let status = response.status();
            let err_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Gemini API returned status {}: {}", status, err_text);
        }

        let gemini_resp: GeminiResponse = response
            .json()
            .await
            .context("Failed to deserialize Gemini API response")?;

        let Some(candidates) = gemini_resp.candidates else {
            return Ok(None);
        };

        let Some(first_candidate) = candidates.into_iter().next() else {
            return Ok(None);
        };

        let Some(parts) = first_candidate.content.and_then(|c| c.parts) else {
            return Ok(None);
        };

        let Some(first_part) = parts.into_iter().next() else {
            return Ok(None);
        };

        let Some(raw_json) = first_part.text else {
            return Ok(None);
        };

        let parsed: GeminiParsedAnime = serde_json::from_str(&raw_json)
            .with_context(|| format!("Failed to parse JSON text from Gemini: {}", raw_json))?;

        if parsed.title.trim().is_empty() || parsed.episode == 0 {
            Ok(None)
        } else {
            Ok(Some(AnimeInfo {
                title: parsed.title.trim().to_string(),
                episode: parsed.episode,
                season: None,
            }))
        }
    }
}
