use anyhow::{Context, Result};
use chrono::Local;
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use super::event::ActivityEvent;

pub struct LogWriter {
    logs_dir: PathBuf,
}

impl LogWriter {
    pub fn new() -> Result<Self> {
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("sys-chronicle")
            .join("logs");

        create_dir_all(&data_dir)
            .with_context(|| format!("Failed to create logs directory at {:?}", data_dir))?;

        Ok(Self { logs_dir: data_dir })
    }

    pub fn get_logs_dir() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("sys-chronicle")
            .join("logs")
    }

    pub fn write_event(&self, event: &ActivityEvent) -> Result<()> {
        let today = Local::now().format("%Y-%m-%d").to_string();
        let log_file_name = format!("activity-{}.jsonl", today);
        let log_path = self.logs_dir.join(log_file_name);

        let json_line = serde_json::to_string(event)?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("Failed to open log file {:?}", log_path))?;

        writeln!(file, "{}", json_line)?;
        Ok(())
    }

    pub fn read_events_for_date(date_str: &str) -> Result<Vec<ActivityEvent>> {
        let logs_dir = Self::get_logs_dir();
        let log_file_name = format!("activity-{}.jsonl", date_str);
        let log_path = logs_dir.join(log_file_name);

        if !log_path.exists() {
            return Ok(Vec::new());
        }

        let content = std::fs::read_to_string(&log_path)?;
        let mut events = Vec::new();

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<ActivityEvent>(line) {
                events.push(event);
            }
        }

        Ok(events)
    }

    pub fn read_recent_events(days: usize) -> Result<Vec<ActivityEvent>> {
        let mut all_events = Vec::new();
        let now = Local::now();

        for i in (0..days).rev() {
            let date = now - chrono::Duration::days(i as i64);
            let date_str = date.format("%Y-%m-%d").to_string();
            if let Ok(events) = Self::read_events_for_date(&date_str) {
                all_events.extend(events);
            }
        }

        Ok(all_events)
    }

    pub fn read_events_for_date_range(start_date: &str, end_date: &str) -> Result<Vec<ActivityEvent>> {
        let start = chrono::NaiveDate::parse_from_str(start_date, "%Y-%m-%d")
            .with_context(|| format!("Invalid start date format: '{}'", start_date))?;
        let end = chrono::NaiveDate::parse_from_str(end_date, "%Y-%m-%d")
            .with_context(|| format!("Invalid end date format: '{}'", end_date))?;

        let mut all_events = Vec::new();
        let mut curr = start;
        while curr <= end {
            let date_str = curr.format("%Y-%m-%d").to_string();
            if let Ok(events) = Self::read_events_for_date(&date_str) {
                all_events.extend(events);
            }
            curr += chrono::Duration::days(1);
        }

        Ok(all_events)
    }
}
