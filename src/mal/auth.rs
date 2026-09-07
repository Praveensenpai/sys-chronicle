use anyhow::{Context, Result};
use chrono::{Duration as ChronoDuration, Local};
use rand::{distributions::Alphanumeric, Rng};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

const MAL_AUTH_URL: &str = "https://myanimelist.net/v1/oauth2/authorize";
const MAL_TOKEN_URL: &str = "https://myanimelist.net/v1/oauth2/token";
const REDIRECT_URI: &str = "http://localhost:8080/callback";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MalConfig {
    pub client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gemini_api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gemini_model: Option<String>,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
}

pub struct MalAuth;

impl MalAuth {
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("sys-chronicle")
            .join("mal_config.json")
    }

    pub fn load_config() -> Result<MalConfig> {
        let path = Self::config_path();
        if !path.exists() {
            anyhow::bail!(
                "MAL config not found at {:?}. Run `sys-chronicle-mal login` first.",
                path
            );
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read MAL config from {:?}", path))?;
        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse MAL config JSON from {:?}", path))
    }

    pub fn save_config(config: &MalConfig) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(config)?;
        fs::write(&path, content)?;
        Ok(())
    }

    pub fn generate_code_verifier() -> String {
        rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(128)
            .map(char::from)
            .collect()
    }

    pub async fn login(
        client_id: String,
        client_secret: Option<String>,
        gemini_api_key: Option<String>,
    ) -> Result<()> {
        let verifier = Self::generate_code_verifier();
        let state: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(16)
            .map(char::from)
            .collect();

        let mut auth_url = Url::parse(MAL_AUTH_URL)?;
        auth_url
            .query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &client_id)
            .append_pair("code_challenge", &verifier)
            .append_pair("code_challenge_method", "plain")
            .append_pair("redirect_uri", REDIRECT_URI)
            .append_pair("state", &state);

        println!("\n[+] Opening browser for MyAnimeList authorization...");
        println!(
            "[+] If browser does not open automatically, visit:\n{}\n",
            auth_url
        );

        let _ = Command::new("xdg-open").arg(auth_url.as_str()).spawn();

        let listener = TcpListener::bind("127.0.0.1:8080")
            .await
            .context("Failed to bind temporary local callback server on 127.0.0.1:8080")?;

        let code = Self::wait_for_auth_code(listener).await?;
        println!("[+] Authorization code received. Exchanging for tokens...");

        let token_resp =
            Self::exchange_code_for_token(&client_id, client_secret.as_deref(), &code, &verifier)
                .await?;
        let expires_at =
            (Local::now() + ChronoDuration::seconds(token_resp.expires_in)).timestamp();

        let config = MalConfig {
            client_id,
            client_secret,
            gemini_api_key,
            gemini_model: Some("gemini-3.5-flash-lite".to_string()),
            access_token: token_resp.access_token,
            refresh_token: token_resp.refresh_token,
            expires_at,
        };

        Self::save_config(&config)?;
        println!(
            "✔ Successfully authenticated and saved credentials to {:?}",
            Self::config_path()
        );
        Ok(())
    }

    async fn wait_for_auth_code(listener: TcpListener) -> Result<String> {
        let (mut socket, _) = listener.accept().await?;
        let mut buffer = [0u8; 2048];
        let n = socket.read(&mut buffer).await?;
        let request = String::from_utf8_lossy(&buffer[..n]);

        let code = Self::extract_code_param(&request).ok_or_else(|| {
            anyhow::anyhow!("Failed to parse authorization code from HTTP callback")
        })?;

        let html =
            "<html><body style='font-family: sans-serif; text-align: center; padding: 50px;'>\
                    <h2>Authentication Successful!</h2>\
                    <p>sys-chronicle-mal is now linked. You may close this tab.</p></body></html>";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            html.len(),
            html
        );
        let _ = socket.write_all(response.as_bytes()).await;

        Ok(code)
    }

    fn extract_code_param(request: &str) -> Option<String> {
        let first_line = request.lines().next()?;
        let path_and_query = first_line.split_whitespace().nth(1)?;
        let parsed_url = Url::parse(&format!("http://localhost{}", path_and_query)).ok()?;
        for (k, v) in parsed_url.query_pairs() {
            if k == "code" {
                return Some(v.to_string());
            }
        }
        None
    }

    async fn exchange_code_for_token(
        client_id: &str,
        client_secret: Option<&str>,
        code: &str,
        verifier: &str,
    ) -> Result<TokenResponse> {
        let client = Client::new();
        let mut params = vec![
            ("client_id", client_id),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", REDIRECT_URI),
        ];

        if let Some(sec) = client_secret {
            if !sec.trim().is_empty() {
                params.push(("client_secret", sec.trim()));
            }
        }

        let res = client.post(MAL_TOKEN_URL).form(&params).send().await?;
        if !res.status().is_success() {
            let err = res.text().await.unwrap_or_default();
            anyhow::bail!("Failed to exchange token: {}", err);
        }

        res.json::<TokenResponse>()
            .await
            .context("Failed to parse token response")
    }

    pub async fn get_valid_token(config: &mut MalConfig) -> Result<String> {
        let now = Local::now().timestamp();
        // Refresh token if within 5 minutes of expiration
        if config.expires_at - now < 300 {
            Self::refresh_access_token(config).await?;
        }
        Ok(config.access_token.clone())
    }

    async fn refresh_access_token(config: &mut MalConfig) -> Result<()> {
        let client = Client::new();
        let mut params = vec![
            ("client_id", config.client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", config.refresh_token.as_str()),
        ];

        if let Some(ref sec) = config.client_secret {
            if !sec.trim().is_empty() {
                params.push(("client_secret", sec.trim()));
            }
        }

        let res = client.post(MAL_TOKEN_URL).form(&params).send().await?;
        if !res.status().is_success() {
            let err = res.text().await.unwrap_or_default();
            anyhow::bail!("Failed to refresh token: {}", err);
        }

        let token_resp: TokenResponse = res.json().await?;
        config.access_token = token_resp.access_token;
        config.refresh_token = token_resp.refresh_token;
        config.expires_at =
            (Local::now() + ChronoDuration::seconds(token_resp.expires_in)).timestamp();
        Self::save_config(config)?;
        Ok(())
    }
}
