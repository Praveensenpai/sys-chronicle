# sys-chronicle ⏱️

**sys-chronicle** is a high-performance, lightweight Rust daemon and CLI tool that logs system activity—active desktop window/application focus timeline, battery charge/discharge events, and CPU/RAM load—into daily rolling JSON Lines files, formatted into AI-digestible Markdown reports.

---

## Features

- 🪟 **Real-time Window Focus Tracking**: Listens to Hyprland Unix socket IPC (`.socket2.sock`) for event-driven focus tracking without CPU polling. Fallback polling for generic Wayland/X11 sessions.
- 🔋 **Power & Battery Monitoring**: Tracks battery levels, charging status (`Charging`, `Discharging`), AC adapter connections, and capacity changes.
- ⚡ **Resource Usage Sampling**: Captures CPU utilization %, RAM used/total, and top resource-consuming applications.
- 🤖 **AI-Optimized Markdown Exporter**: `sys-chronicle export` aggregates activity logs into compressed Markdown reports for feeding into LLMs (Claude, ChatGPT, Antigravity, Gemini).
- ⚙️ **Systemd Integration**: `sys-chronicle install-service` automatically configures a `systemd --user` unit.

---

## Installation

### One-liner Remote Install
```bash
curl -sSL https://raw.githubusercontent.com/Praveensenpai/sys-chronicle/main/install.sh | bash
```

### From Source
```bash
git clone https://github.com/Praveensenpai/sys-chronicle.git
cd sys-chronicle
cargo build --release
cp target/release/sys-chronicle ~/.local/bin/
```

---

## CLI Usage

```bash
# View instant status snapshot (Active app, battery %, CPU/RAM)
sys-chronicle status

# Export activity report for today formatted for AI ingestion
sys-chronicle export

# Export activity report for a specific date
sys-chronicle export --date 2026-08-10

# Run daemon in foreground
sys-chronicle daemon --interval 30

# Install systemd user service
sys-chronicle install-service
```

---

## Systemd User Service

Enable and start background logging:
```bash
systemctl --user enable --now sys-chronicle.service
```

Logs are stored in:
`~/.local/share/sys-chronicle/logs/activity-YYYY-MM-DD.jsonl`

---

## License

MIT License
