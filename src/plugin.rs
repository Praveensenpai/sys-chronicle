use anyhow::{Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

use crate::logger::ActivityEvent;

pub struct PluginDispatcher;

impl PluginDispatcher {
    pub fn get_plugins_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("sys-chronicle")
            .join("plugins")
    }

    pub fn ensure_plugins_dir() -> Result<PathBuf> {
        let dir = Self::get_plugins_dir();
        if !dir.exists() {
            fs::create_dir_all(&dir)
                .with_context(|| format!("Failed to create plugins directory at {:?}", dir))?;
        }
        Ok(dir)
    }

    pub fn list_plugins() -> Vec<PathBuf> {
        let dir = Self::get_plugins_dir();
        let Ok(entries) = fs::read_dir(&dir) else {
            return Vec::new();
        };

        let mut plugins = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if Self::is_executable(&path) {
                plugins.push(path);
            }
        }
        plugins.sort();
        plugins
    }

    fn is_executable(path: &Path) -> bool {
        if !path.is_file() {
            return false;
        }

        fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    pub fn dispatch_event(event: &ActivityEvent) {
        let Ok(payload) = serde_json::to_vec(event) else {
            return;
        };

        let plugins = Self::list_plugins();
        if plugins.is_empty() {
            return;
        }

        tokio::spawn(async move {
            for plugin in plugins {
                let payload_clone = payload.clone();
                tokio::spawn(async move {
                    let _ = Self::run_plugin(&plugin, payload_clone).await;
                });
            }
        });
    }

    async fn run_plugin(plugin_path: &Path, payload: Vec<u8>) -> Result<()> {
        let mut child = Command::new(plugin_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("Failed to spawn plugin at {:?}", plugin_path))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(&payload).await?;
            stdin.flush().await?;
        }

        let wait_result = timeout(Duration::from_secs(30), child.wait_with_output()).await;
        match wait_result {
            Ok(Ok(output)) => {
                if !output.status.success() {
                    let err = String::from_utf8_lossy(&output.stderr);
                    eprintln!(
                        "[Plugin] {:?} exited with code {:?}: {}",
                        plugin_path.file_name().unwrap_or_default(),
                        output.status.code(),
                        err.trim()
                    );
                }
            }
            Ok(Err(e)) => {
                eprintln!("[Plugin] Error waiting on {:?}: {}", plugin_path, e);
            }
            Err(_) => {
                eprintln!("[Plugin] Timed out waiting on {:?}", plugin_path);
            }
        }

        Ok(())
    }
}
