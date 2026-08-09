mod exporter;
mod logger;
mod monitor;
mod service;
mod tui;

use anyhow::Result;
use chrono::Local;
use clap::{Parser, Subcommand};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use sysinfo::System;
use tokio::signal;

use exporter::generate_ai_report;
use logger::LogWriter;
use monitor::{MetricsMonitor, PowerMonitor, WindowMonitor};

#[derive(Parser)]
#[command(
    name = "sys-chronicle",
    author = "Praveensenpai",
    version = "0.3.1",
    about = "Timestamped system activity logger (apps, power, CPU/RAM) for AI analysis"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run background daemon logging window activity, battery state, and system metrics
    Daemon {
        /// System metrics sample interval in seconds
        #[arg(short, long, default_value_t = 5)]
        interval: u64,
    },
    /// Show interactive live Ratatui TUI dashboard of desktop focus, power supply, and resource load
    Status {
        /// Output plain text snapshot instead of launching Ratatui TUI
        #[arg(short, long)]
        plain: bool,
    },
    /// View summary of recorded activity
    Summary {
        /// Specific date (YYYY-MM-DD), default is today
        #[arg(short, long)]
        date: Option<String>,
        /// Number of recent days to include
        #[arg(short, long, default_value_t = 1)]
        days: usize,
    },
    /// Export activity logs as a structured Markdown prompt for AI analysis
    Export {
        /// Specific date (YYYY-MM-DD), default is today
        #[arg(short, long)]
        date: Option<String>,
        /// Number of recent days to include
        #[arg(short, long, default_value_t = 1)]
        days: usize,
    },
    /// Generate and enable systemd --user service
    InstallService,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Daemon { interval } => {
            println!("[sys-chronicle daemon v0.3.1 starting]");
            println!("[+] Logging to: {:?}", LogWriter::get_logs_dir());

            let running = Arc::new(AtomicBool::new(true));
            let r_win = Arc::clone(&running);
            let r_pow = Arc::clone(&running);
            let r_met = Arc::clone(&running);

            let w_writer = LogWriter::new()?;
            let p_writer = LogWriter::new()?;
            let m_writer = LogWriter::new()?;

            let mut window_mon = WindowMonitor::new(w_writer);
            let mut power_mon = PowerMonitor::new(p_writer);
            let mut metrics_mon = MetricsMonitor::new(m_writer, interval);

            let h_win = tokio::spawn(async move {
                let _ = window_mon.run(r_win).await;
            });
            let h_pow = tokio::spawn(async move {
                let _ = power_mon.run(r_pow).await;
            });
            let h_met = tokio::spawn(async move {
                let _ = metrics_mon.run(r_met).await;
            });

            println!("[+] Monitors initialized. Press Ctrl+C to stop.");
            signal::ctrl_c().await?;
            println!("\n[!] Shutdown signal received, terminating daemon...");

            running.store(false, Ordering::SeqCst);
            let _ = tokio::join!(h_win, h_pow, h_met);
            println!("[+] Daemon stopped gracefully.");
        }
        Commands::Status { plain } => {
            if plain {
                println!("=== SysChronicle Status ===");
                if let Some((cls, title)) = WindowMonitor::get_current_window() {
                    println!("Active Window: {} (\"{}\")", cls, title);
                } else {
                    println!("Active Window: None / Idle");
                }

                if let Some(pow) = PowerMonitor::read_current_state() {
                    let ac_str = if pow.ac_online { "Plugged" } else { "Unplugged" };
                    println!("Battery: {}% ({}, {})", pow.capacity, pow.status, ac_str);
                } else {
                    println!("Battery: Unknown / Desktop");
                }

                let mut sys = System::new_all();
                let metrics = MetricsMonitor::sample_metrics(&mut sys);
                println!(
                    "CPU Load: {:.1}% | RAM: {:.1}% ({} / {} MB)",
                    metrics.cpu_pct, metrics.ram_pct, metrics.ram_used_mb, metrics.ram_total_mb
                );
                println!("Top Applications: {}", metrics.top_apps.join(", "));
            } else {
                tui::run_status_tui()?;
            }
        }
        Commands::Summary { date, days } => {
            let title_date = date
                .clone()
                .unwrap_or_else(|| Local::now().format("%Y-%m-%d").to_string());
            let events = if let Some(d) = date {
                LogWriter::read_events_for_date(&d)?
            } else {
                LogWriter::read_recent_events(days)?
            };

            let report = generate_ai_report(&events, &title_date);
            println!("{}", report);
        }
        Commands::Export { date, days } => {
            let title_date = date
                .clone()
                .unwrap_or_else(|| Local::now().format("%Y-%m-%d").to_string());
            let events = if let Some(d) = date {
                LogWriter::read_events_for_date(&d)?
            } else {
                LogWriter::read_recent_events(days)?
            };

            let report = generate_ai_report(&events, &title_date);
            println!("{}", report);
        }
        Commands::InstallService => {
            service::install_user_service()?;
        }
    }

    Ok(())
}
