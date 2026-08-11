use anyhow::Result;
use chrono::Local;
use serde::Deserialize;
use std::env;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;
use tokio::time::{sleep, Duration};

use crate::logger::{ActivityEvent, LogWriter};

#[derive(Debug, Deserialize, Default)]
struct HyprActiveWindow {
    class: String,
    title: String,
}

pub struct WindowMonitor {
    writer: LogWriter,
    last_window: Option<(String, String, chrono::DateTime<Local>)>,
}

impl WindowMonitor {
    pub fn new(writer: LogWriter) -> Self {
        Self {
            writer,
            last_window: None,
        }
    }

    pub fn get_current_window() -> Option<(String, String)> {
        // Ensure HYPRLAND_INSTANCE_SIGNATURE is set if possible
        if env::var("HYPRLAND_INSTANCE_SIGNATURE").is_err() {
            if let Some((sig, _)) = Self::find_hyprland_socket_and_sig() {
                env::set_var("HYPRLAND_INSTANCE_SIGNATURE", &sig);
            }
        }

        let mut cmd = Command::new("hyprctl");
        cmd.arg("activewindow").arg("-j");

        if let Ok(sig) = env::var("HYPRLAND_INSTANCE_SIGNATURE") {
            cmd.env("HYPRLAND_INSTANCE_SIGNATURE", sig);
        }

        let output = cmd.output().ok()?;

        if !output.status.success() {
            return None;
        }

        let win: HyprActiveWindow = serde_json::from_slice(&output.stdout).ok()?;
        if win.class.is_empty() && win.title.is_empty() {
            None
        } else {
            Some((win.class, win.title))
        }
    }

    pub async fn run(&mut self, running: Arc<AtomicBool>) -> Result<()> {
        while running.load(Ordering::SeqCst) {
            let socket_info = Self::find_hyprland_socket_and_sig();

            if let Some((sig, path)) = socket_info {
                env::set_var("HYPRLAND_INSTANCE_SIGNATURE", &sig);

                if let Ok(stream) = UnixStream::connect(&path).await {
                    println!("[WindowMonitor] Listening to Hyprland IPC socket: {:?}", path);
                    let reader = BufReader::new(stream);
                    let mut lines = reader.lines();

                    // Log initial window state
                    if let Some((cls, title)) = Self::get_current_window() {
                        self.on_window_change(cls, title);
                    }

                    let mut ipc_error = false;
                    while running.load(Ordering::SeqCst) {
                        tokio::select! {
                            line = lines.next_line() => {
                                match line {
                                    Ok(Some(msg)) => {
                                        if msg.starts_with("activewindow>>") {
                                            let payload = &msg["activewindow>>".len()..];
                                            let parts: Vec<&str> = payload.splitn(2, ',').collect();
                                            let app_class = parts.first().unwrap_or(&"").to_string();
                                            let title = parts.get(1).unwrap_or(&"").to_string();

                                            if !app_class.is_empty() || !title.is_empty() {
                                                self.on_window_change(app_class, title);
                                            }
                                        }
                                    }
                                    Ok(None) | Err(_) => {
                                        ipc_error = true;
                                        break;
                                    }
                                }
                            }
                            _ = sleep(Duration::from_secs(5)) => {
                                if let Some((cls, title)) = Self::get_current_window() {
                                    self.on_window_change(cls, title);
                                }
                            }
                        }
                    }

                    if !running.load(Ordering::SeqCst) {
                        self.finish_current_window();
                        return Ok(());
                    }

                    if ipc_error {
                        eprintln!("[WindowMonitor] Hyprland IPC connection closed. Will retry/poll.");
                    }
                }
            }

            // Fallback polling mode for 5 seconds before checking socket again
            for _ in 0..5 {
                if !running.load(Ordering::SeqCst) {
                    self.finish_current_window();
                    return Ok(());
                }

                if let Some((cls, title)) = Self::get_current_window() {
                    let is_different = match &self.last_window {
                        Some((old_cls, old_title, _)) => old_cls != &cls || old_title != &title,
                        None => true,
                    };
                    if is_different {
                        self.on_window_change(cls, title);
                    }
                }
                sleep(Duration::from_secs(2)).await;
            }
        }

        self.finish_current_window();
        Ok(())
    }

    fn on_window_change(&mut self, new_class: String, new_title: String) {
        if matches!(
            self.last_window.as_ref(),
            Some((old_class, old_title, _)) if old_class == &new_class && old_title == &new_title
        ) {
            return;
        }

        let now = Local::now();

        if let Some((old_class, old_title, start_time)) = self.last_window.take() {
            let duration_secs = (now - start_time).num_seconds().max(0) as u64;

            let prev_event = ActivityEvent::WindowFocus {
                timestamp: start_time.format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string(),
                app_class: old_class,
                title: old_title,
                duration_secs: Some(duration_secs),
            };
            let _ = self.writer.write_event(&prev_event);
        }

        let timestamp_str = now.format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string();
        let new_event = ActivityEvent::WindowFocus {
            timestamp: timestamp_str,
            app_class: new_class.clone(),
            title: new_title.clone(),
            duration_secs: None,
        };
        let _ = self.writer.write_event(&new_event);

        self.last_window = Some((new_class, new_title, now));
    }

    fn finish_current_window(&mut self) {
        let Some((app_class, title, start_time)) = self.last_window.take() else {
            return;
        };

        let duration_secs = (Local::now() - start_time).num_seconds().max(0) as u64;
        let event = ActivityEvent::WindowFocus {
            timestamp: start_time.format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string(),
            app_class,
            title,
            duration_secs: Some(duration_secs),
        };
        let _ = self.writer.write_event(&event);
    }

    pub fn find_hyprland_socket_and_sig() -> Option<(String, PathBuf)> {
        let xdg_runtime = env::var("XDG_RUNTIME_DIR")
            .or_else(|_| dirs::runtime_dir().map(|p| p.to_string_lossy().to_string()).ok_or(env::VarError::NotPresent))
            .ok()?;

        // 1. Try env var first
        if let Ok(instance) = env::var("HYPRLAND_INSTANCE_SIGNATURE") {
            let socket_path = PathBuf::from(&xdg_runtime)
                .join("hypr")
                .join(&instance)
                .join(".socket2.sock");
            if socket_path.exists() && StdUnixStream::connect(&socket_path).is_ok() {
                return Some((instance, socket_path));
            }
        }

        // 2. Scan $XDG_RUNTIME_DIR/hypr/ for active hyprland instance signature directory
        let hypr_dir = PathBuf::from(&xdg_runtime).join("hypr");
        if let Ok(entries) = std::fs::read_dir(&hypr_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let socket_path = path.join(".socket2.sock");
                    if socket_path.exists() && StdUnixStream::connect(&socket_path).is_ok() {
                        if let Some(sig) = path.file_name().and_then(|s| s.to_str()) {
                            return Some((sig.to_string(), socket_path));
                        }
                    }
                }
            }
        }

        None
    }
}
