use anyhow::{Context, Result};
use chrono::{DateTime, Local, TimeZone};
use clap::{Parser, Subcommand};
use std::env;
use std::io::{self, IsTerminal, Write};
use sys_chronicle::mal::{AnimeParser, GeminiParser, MalAuth, MalClient, MalHandler};

#[derive(Parser)]
#[command(
    name = "sys-chronicle-mal",
    author = "Praveensenpai",
    version = env!("CARGO_PKG_VERSION"),
    about = "MyAnimeList Companion Scrobbler & Plugin for sys-chronicle"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Authenticate with MyAnimeList using OAuth2 PKCE
    Login,
    /// View current MAL authentication and profile status
    Status,
    /// Test anime title and episode parsing on a sample filename
    Test {
        /// File path or media title to test
        name: String,
    },
    /// Manually sync an anime episode to MyAnimeList
    Sync {
        /// File path or media title
        name: String,
        /// Force update even if MAL progress is already ahead
        #[arg(long, short)]
        force: bool,
    },
    /// View or configure MAL settings (e.g. toast duration)
    Config {
        /// Set toast notification duration in seconds (e.g. 2)
        #[arg(long, short)]
        toast: Option<u32>,
        /// Positional toast notification duration in seconds
        seconds: Option<u32>,
    },
    /// Hook mode (reads ActivityEvent JSON from stdin)
    Hook,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Login) => {
            run_login().await?;
        }
        Some(Commands::Status) => {
            run_status().await?;
        }
        Some(Commands::Test { name }) => {
            run_test(&name).await?;
        }
        Some(Commands::Sync { name, force }) => {
            run_sync(&name, force).await?;
        }
        Some(Commands::Config { toast, seconds }) => {
            run_config(toast.or(seconds)).await?;
        }
        Some(Commands::Hook) | None => {
            if io::stdin().is_terminal() && cli.command.is_none() {
                println!("sys-chronicle-mal v{}", env!("CARGO_PKG_VERSION"));
                println!("Usage: sys-chronicle-mal [login | status | test <file> | hook]");
                println!("To run as a plugin, symlink to ~/.config/sys-chronicle/plugins/sys-chronicle-mal");
                return Ok(());
            }
            MalHandler::handle_stdin_hook().await?;
        }
    }

    Ok(())
}

async fn run_login() -> Result<()> {
    println!("=== MyAnimeList OAuth2 PKCE Setup ===");
    println!("To connect, visit https://myanimelist.net/apiconfig to view your app details.");
    println!("(Redirect URI in your app settings MUST be: 'http://localhost:8080/callback')\n");

    print!("Enter MyAnimeList Client ID (32 chars): ");
    io::stdout().flush()?;
    let mut client_id = String::new();
    io::stdin().read_line(&mut client_id)?;
    let mut client_id = client_id.trim().to_string();
    if client_id.is_empty() {
        anyhow::bail!("Client ID cannot be empty.");
    }

    let mut client_secret: Option<String> = None;

    if client_id.len() == 64 {
        println!("\n[!] Notice: You entered a 64-character key.");
        println!("[!] On MyAnimeList, the 64-character key is the Client Secret.");
        println!("[!] The Client ID is the 32-character key shown directly above it on MAL.");
        client_secret = Some(client_id);

        print!("\nEnter MyAnimeList Client ID (32 chars): ");
        io::stdout().flush()?;
        let mut real_id = String::new();
        io::stdin().read_line(&mut real_id)?;
        client_id = real_id.trim().to_string();
        if client_id.is_empty() {
            anyhow::bail!("Client ID cannot be empty.");
        }
    }

    if client_secret.is_none() {
        print!("Enter MyAnimeList Client Secret (64 chars, from apiconfig): ");
        io::stdout().flush()?;
        let mut secret = String::new();
        io::stdin().read_line(&mut secret)?;
        let secret = secret.trim();
        if !secret.is_empty() {
            client_secret = Some(secret.to_string());
        }
    }

    print!("\nEnter Gemini API Key (optional, press Enter to skip): ");
    io::stdout().flush()?;
    let mut gemini_key = String::new();
    io::stdin().read_line(&mut gemini_key)?;
    let gemini_key = gemini_key.trim();
    let gemini_api_key = if gemini_key.is_empty() {
        None
    } else {
        Some(gemini_key.to_string())
    };

    MalAuth::login(client_id, client_secret, gemini_api_key).await
}

async fn run_status() -> Result<()> {
    let mut config = MalAuth::load_config().context("Could not load MAL configuration")?;
    println!("=== sys-chronicle-mal Status ===");
    println!("Config file: {:?}", MalAuth::config_path());
    println!("Client ID: {}", config.client_id);

    let gemini_status = if let Some(ref k) = config.gemini_api_key {
        format!(
            "Configured (ending in ...{})",
            &k[k.len().saturating_sub(4)..]
        )
    } else if env::var("GEMINI_API_KEY").is_ok() {
        "Configured via GEMINI_API_KEY environment variable".to_string()
    } else {
        "Not configured (using local regex parser)".to_string()
    };
    println!("Gemini AI Parser: {}", gemini_status);

    let exp_dt: DateTime<Local> = Local
        .timestamp_opt(config.expires_at, 0)
        .single()
        .unwrap_or_else(Local::now);
    println!("Token Expires: {}", exp_dt.format("%Y-%m-%d %H:%M:%S %Z"));

    let token = MalAuth::get_valid_token(&mut config).await?;
    let client = MalClient::new(token)?;
    match client.get_current_user().await {
        Ok(user) => {
            println!("Logged-in User: {} (ID: {})", user.name, user.id);
            println!("✔ MAL Connection: Healthy");
        }
        Err(e) => {
            println!("✖ Failed to query MAL profile: {}", e);
        }
    }

    Ok(())
}

async fn run_test(name: &str) -> Result<()> {
    println!("Testing anime title & episode extraction for: \"{}\"", name);

    let config = MalAuth::load_config().ok();
    let gemini_key = env::var("GEMINI_API_KEY")
        .ok()
        .or_else(|| config.as_ref().and_then(|c| c.gemini_api_key.clone()));

    if let Some(key) = gemini_key {
        println!("[+] Querying Gemini AI parser...");
        let model = config.as_ref().and_then(|c| c.gemini_model.clone());
        if let Ok(gemini) = GeminiParser::new(key, model) {
            match gemini.parse(name).await {
                Ok(Some(info)) => {
                    println!(
                        "✔ Gemini Result: \"{}\" Episode {}",
                        info.title, info.episode
                    );
                    return Ok(());
                }
                Ok(None) => println!("[-] Gemini returned no match, trying regex..."),
                Err(e) => println!("[-] Gemini failed ({}), falling back to regex...", e),
            }
        }
    } else {
        println!("[-] Gemini API key not found, using regex parser...");
    }

    match AnimeParser::parse(name) {
        Some(info) => {
            println!(
                "✔ Regex Result: \"{}\" Episode {}",
                info.title, info.episode
            );
        }
        None => {
            println!("✖ Regex could not extract title or episode.");
        }
    }

    Ok(())
}

async fn run_sync(name: &str, force: bool) -> Result<()> {
    println!("Manually syncing: \"{}\" (force: {})", name, force);
    match MalHandler::sync_media_with_options(name, 0, 0, force).await? {
        Some(record) => {
            let (badge, _) = record.status.badge();
            println!(
                "Status: {} ({} Ep {})",
                badge, record.canonical_title, record.episode
            );
        }
        None => {
            println!("[-] Sync could not be completed.");
        }
    }
    Ok(())
}

async fn run_config(toast_timeout: Option<u32>) -> Result<()> {
    let mut config = MalAuth::load_config()?;
    if let Some(secs) = toast_timeout {
        config.toast_timeout_secs = Some(secs);
        MalAuth::save_config(&config)?;
        println!(
            "✔ Toast notification duration set to {}s ({}ms).",
            secs,
            secs * 1000
        );
    } else {
        println!("=== MyAnimeList Configuration ===");
        println!("Config file:    {:?}", MalAuth::config_path());
        println!("Client ID:      {}", config.client_id);
        println!(
            "Toast duration: {}s (default: 2s)",
            config.toast_timeout_secs.unwrap_or(2)
        );
        println!(
            "\nUsage to change: sys-chronicle-mal config <seconds> (e.g. `sys-chronicle-mal config 2`)"
        );
    }
    Ok(())
}
