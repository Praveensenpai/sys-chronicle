# sys-chronicle ⏱️

**sys-chronicle** is a high-performance, lightweight Rust daemon and CLI tool that logs system activity—active desktop window/application focus timeline, battery charge/discharge events, and CPU/RAM load—into daily rolling JSON Lines files, formatted into AI-digestible Markdown reports.

---

## Features

- 🪟 **Real-time Window Focus Tracking**: Listens to Hyprland Unix socket IPC (`.socket2.sock`) for event-driven focus tracking without CPU polling. Fallback polling for generic Wayland/X11 sessions.
- 🔋 **Power & Battery Monitoring**: Tracks battery levels, charging status (`Charging`, `Discharging`), AC adapter connections, and capacity changes.
- ⚡ **Resource Usage Sampling**: Captures CPU utilization %, RAM used/total, and top resource-consuming applications.
- 🤖 **AI-Optimized Markdown Exporter**: `sys-chronicle export` aggregates activity logs into compressed Markdown reports for feeding into LLMs (Claude, ChatGPT, Antigravity, Gemini).
- ⚙️ **Systemd Integration**: `sys-chronicle install-service` automatically configures and starts a `systemd --user` unit.

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

# Copy today's report directly to your clipboard (Wayland)
sys-chronicle export | wl-copy

# Export activity report for a specific date
sys-chronicle export --date 2026-08-10

# Export past N days
sys-chronicle export --days 3

# Run daemon in foreground (default interval 5s)
sys-chronicle daemon --interval 5

# Install & enable systemd user service
sys-chronicle install-service
```

---

## Copying Context to AI Workflow

Run the following command to copy your formatted daily timeline directly to your clipboard:

```bash
sys-chronicle export | wl-copy
```

Then paste into any AI model with a prompt like:
> *"Analyze my system activity log from today (`sys-chronicle export`). Summarize my application usage timeline, highlight battery discharge periods, and identify resource load spikes."*

---

## Systemd User Service

The background service automatically starts upon installation:
```bash
systemctl --user status sys-chronicle.service
```

Logs are stored in:
`~/.local/share/sys-chronicle/logs/activity-YYYY-MM-DD.jsonl`

---

## License

MIT License
