use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const REPO: &str = "Praveensenpai/sys-chronicle";
const TARBALL_ASSET: &str = "sys-chronicle-linux-x86_64.tar.gz";

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub struct SemVer {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl SemVer {
    pub fn parse(s: &str) -> Option<Self> {
        let clean = s.trim().trim_start_matches('v');
        let parts: Vec<&str> = clean.split('.').collect();
        if parts.len() < 3 {
            return None;
        }
        let major = parts[0].parse().ok()?;
        let minor = parts[1].parse().ok()?;
        let patch_token = parts[2].split(['-', '+']).next()?;
        let patch = patch_token.parse().ok()?;
        Some(Self {
            major,
            minor,
            patch,
        })
    }

    pub fn is_newer_than(&self, other: &Self) -> bool {
        self > other
    }
}

#[derive(Debug, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
}

#[derive(Debug, Deserialize)]
pub struct ReleaseResponse {
    pub tag_name: String,
    pub assets: Vec<ReleaseAsset>,
}

pub async fn fetch_latest_release(repo: &str) -> Result<ReleaseResponse> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let user_agent = format!("sys-chronicle/{}", env!("CARGO_PKG_VERSION"));

    let client = reqwest::Client::builder()
        .user_agent(user_agent)
        .build()
        .context("Failed to construct HTTP client for updater")?;

    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .context("Failed to query GitHub Releases API")?
        .error_for_status()
        .context("GitHub Releases API returned non-success HTTP status")?;

    let release: ReleaseResponse = resp
        .json()
        .await
        .context("Failed to deserialize GitHub release payload")?;

    Ok(release)
}

fn determine_install_dir() -> Result<PathBuf> {
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            if parent.is_dir() && parent.file_name().and_then(|n| n.to_str()) == Some("bin") {
                return Ok(parent.to_path_buf());
            }
        }
    }

    let home = dirs::home_dir().context("Could not determine user home directory")?;
    let bin_dir = home.join(".local").join("bin");
    fs::create_dir_all(&bin_dir).context("Failed to create ~/.local/bin directory")?;
    Ok(bin_dir)
}

fn install_binary(src_file: &Path, dst_file: &Path) -> Result<()> {
    if dst_file.exists() {
        let _ = fs::remove_file(dst_file);
    }

    fs::copy(src_file, dst_file).with_context(|| {
        format!(
            "Failed to copy {} to {}",
            src_file.display(),
            dst_file.display()
        )
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o755);
        fs::set_permissions(dst_file, perms)
            .with_context(|| format!("Failed to set permissions on {}", dst_file.display()))?;
    }

    Ok(())
}

fn reload_service_and_plugin(install_dir: &Path) {
    let mal_bin = install_dir.join("sys-chronicle-mal");
    if mal_bin.exists() {
        if let Some(home) = dirs::home_dir() {
            let plugin_dir = home.join(".config").join("sys-chronicle").join("plugins");
            if fs::create_dir_all(&plugin_dir).is_ok() {
                let link_path = plugin_dir.join("sys-chronicle-mal");
                let _ = fs::remove_file(&link_path);
                #[cfg(unix)]
                let _ = std::os::unix::fs::symlink(&mal_bin, &link_path);
            }
        }
    }

    let _ = Command::new("systemctl")
        .args(["--user", "try-restart", "sys-chronicle.service"])
        .status();
}

async fn download_and_extract(url: &str, temp_dir: &Path) -> Result<()> {
    let client = reqwest::Client::builder()
        .user_agent(format!("sys-chronicle/{}", env!("CARGO_PKG_VERSION")))
        .build()?;

    let bytes = client
        .get(url)
        .send()
        .await
        .context("Failed to download release archive")?
        .error_for_status()
        .context("HTTP error during archive download")?
        .bytes()
        .await
        .context("Failed to read archive bytes")?;

    let tar_gz_path = temp_dir.join(TARBALL_ASSET);
    fs::write(&tar_gz_path, bytes).context("Failed to write downloaded archive to disk")?;

    let status = Command::new("tar")
        .args([
            "-xzf",
            tar_gz_path.to_str().unwrap_or_default(),
            "-C",
            temp_dir.to_str().unwrap_or_default(),
        ])
        .status()
        .context("Failed to execute tar command to extract archive")?;

    if !status.success() {
        bail!("tar command failed with non-zero exit status");
    }

    Ok(())
}

pub async fn run_update(check_only: bool, force: bool) -> Result<()> {
    let current_raw = env!("CARGO_PKG_VERSION");
    let current_ver = SemVer::parse(current_raw)
        .context("Failed to parse current CARGO_PKG_VERSION as SemVer")?;

    println!(
        "[+] Checking for updates from GitHub (current: v{})...",
        current_raw
    );
    let release = fetch_latest_release(REPO).await?;
    let remote_ver =
        SemVer::parse(&release.tag_name).context("Failed to parse remote release tag_name")?;

    let is_upgrade = remote_ver.is_newer_than(&current_ver);
    if !is_upgrade && !force {
        println!(
            "✔ sys-chronicle is already up to date ({}).",
            release.tag_name
        );
        return Ok(());
    }

    if check_only {
        if is_upgrade {
            println!(
                "💡 A new version is available: v{} -> {} (run `sys-chronicle update` to upgrade)",
                current_raw, release.tag_name
            );
        } else {
            println!(
                "✔ Current version v{} matches latest release {}.",
                current_raw, release.tag_name
            );
        }
        return Ok(());
    }

    let asset = release
        .assets
        .iter()
        .find(|a| a.name == TARBALL_ASSET)
        .with_context(|| {
            format!(
                "Release {} missing required asset {}",
                release.tag_name, TARBALL_ASSET
            )
        })?;

    let temp_dir =
        std::env::temp_dir().join(format!("sys-chronicle-update-{}", std::process::id()));
    fs::create_dir_all(&temp_dir)?;

    let result = perform_upgrade(
        &asset.browser_download_url,
        &temp_dir,
        &release.tag_name,
        current_raw,
    )
    .await;
    let _ = fs::remove_dir_all(&temp_dir);
    result
}

async fn perform_upgrade(
    download_url: &str,
    temp_dir: &Path,
    new_tag: &str,
    current_raw: &str,
) -> Result<()> {
    println!("[+] Downloading {}...", download_url);
    download_and_extract(download_url, temp_dir).await?;

    let install_dir = determine_install_dir()?;
    println!(
        "[+] Installing updated binaries to {}...",
        install_dir.display()
    );

    let sys_bin = temp_dir.join("sys-chronicle");
    if sys_bin.exists() {
        install_binary(&sys_bin, &install_dir.join("sys-chronicle"))?;
        println!("✔ Updated sys-chronicle");
    }

    let mal_bin = temp_dir.join("sys-chronicle-mal");
    if mal_bin.exists() {
        install_binary(&mal_bin, &install_dir.join("sys-chronicle-mal"))?;
        println!("✔ Updated sys-chronicle-mal (plugin linked)");
    }

    reload_service_and_plugin(&install_dir);
    println!(
        "🎉 Successfully updated sys-chronicle from v{} to {}!",
        current_raw, new_tag
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semver_parse_clean_and_tags() {
        let v1 = SemVer::parse("v0.7.0").expect("parse v0.7.0");
        assert_eq!(
            v1,
            SemVer {
                major: 0,
                minor: 7,
                patch: 0
            }
        );

        let v2 = SemVer::parse("0.8.1-beta").expect("parse with pre-release");
        assert_eq!(
            v2,
            SemVer {
                major: 0,
                minor: 8,
                patch: 1
            }
        );

        assert!(SemVer::parse("invalid").is_none());
    }

    #[test]
    fn test_semver_ordering_comparison() {
        let v1 = SemVer::parse("v0.7.0").expect("v1");
        let v2 = SemVer::parse("v0.7.1").expect("v2");
        let v3 = SemVer::parse("v0.8.0").expect("v3");
        let v4 = SemVer::parse("v1.0.0").expect("v4");

        assert!(v2.is_newer_than(&v1));
        assert!(v3.is_newer_than(&v2));
        assert!(v4.is_newer_than(&v3));
        assert!(!v1.is_newer_than(&v2));
        assert!(!v1.is_newer_than(&v1));
    }
}
