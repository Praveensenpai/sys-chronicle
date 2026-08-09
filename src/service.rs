use anyhow::{Context, Result};
use std::fs::{create_dir_all, write};
use std::process::Command;

pub fn install_user_service() -> Result<()> {
    let home = dirs::home_dir().context("Could not find home directory")?;
    let service_dir = home.join(".config").join("systemd").join("user");
    create_dir_all(&service_dir)?;

    let service_file = service_dir.join("sys-chronicle.service");
    let exec_path = home.join(".local").join("bin").join("sys-chronicle");

    let unit_content = format!(
        r#"[Unit]
Description=SysChronicle System Activity & Metrics Logger
After=graphical-session.target

[Service]
Type=simple
ExecStart={} daemon --interval 5
Restart=on-failure
RestartSec=5s

[Install]
WantedBy=default.target
"#,
        exec_path.display()
    );

    write(&service_file, unit_content)?;
    println!("[+] Created systemd user unit at {:?}", service_file);

    let reload = Command::new("systemctl")
        .arg("--user")
        .arg("daemon-reload")
        .status();

    if reload.is_ok() {
        println!("[+] Executed systemctl --user daemon-reload");
    }

    let enable = Command::new("systemctl")
        .arg("--user")
        .arg("enable")
        .arg("--now")
        .arg("sys-chronicle.service")
        .status();

    if enable.is_ok() {
        println!("✔ Systemd user service enabled & started (sys-chronicle.service)");
    }

    Ok(())
}
