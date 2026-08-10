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
        let output = Command::new("hyprctl")
            .arg("activewindow")
            .arg("-j")
            .output()
            .ok()?;

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
        let socket_path = Self::find_hyprland_socket();

        if let Some(path) = socket_path {
            if let Ok(stream) = UnixStream::connect(&path).await {
                println!("[WindowMonitor] Listening to Hyprland IPC socket: {:?}", path);
                let reader = BufReader::new(stream);
                let mut lines = reader.lines();

                // Log initial window state
                if let Some((cls, title)) = Self::get_current_window() {
                    self.on_window_change(cls, title);
                }

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
                                Ok(None) | Err(_) => break,
                            }
                        }
                        // The event stream is normally authoritative, but a periodic query
                        // recovers a focus change when Hyprland drops an IPC event.
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

                eprintln!("[WindowMonitor] Hyprland IPC connection closed. Falling back to polling.");
            }
        }

        // Fallback polling mode
        println!("[WindowMonitor] Hyprland IPC socket unavailable. Using polling mode (2s interval).");
        while running.load(Ordering::SeqCst) {
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

        self.finish_current_window();

        Ok(())
    }

    fn on_window_change(&mut self, new_class: String, new_title: String) {
        // Hyprland can send the current active window immediately after a
        // subscription connects. Do not turn that duplicate into a zero-second
        // session or reset its start time.
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

    fn find_hyprland_socket() -> Option<PathBuf> {
        let xdg_runtime = env::var("XDG_RUNTIME_DIR").ok()?;
        let instance = env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
        let socket_path = PathBuf::from(xdg_runtime)
            .join("hypr")
            .join(instance)
            .join(".socket2.sock");

        if socket_path.exists() && StdUnixStream::connect(&socket_path).is_ok() {
            Some(socket_path)
        } else {
            None
        }
    }
}
