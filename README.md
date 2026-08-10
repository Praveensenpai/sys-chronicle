# sys-chronicle ⏱️

> **High-Performance System Activity Logger, Interactive TUI Explorer & AI Context Generator for Arch Linux & Wayland**

[![Release](https://img.shields.io/github/v/release/Praveensenpai/sys-chronicle?color=blue&style=flat-square)](https://github.com/Praveensenpai/sys-chronicle/releases)
[![License](https://img.shields.io/badge/license-MIT-green.style=flat-square)](LICENSE)

`sys-chronicle` is an ultra-lightweight (~10 MB RAM) Rust background daemon and feature-packed Ratatui TUI dashboard that tracks active window focus, system power states, CPU/RAM utilization, and process metrics into daily JSON Lines logs, formatted into instant AI-digestible Markdown reports.

---

## ❓ The Problem & Why You Need `sys-chronicle`

1. **"What did I actually spend time on today?"**
   - Traditional system monitors (`htop`, `btop`) show instant CPU spikes, but leave no persistent record of your actual window focus history or screen time throughout the day.
2. **AI Assistance Needs High-Fidelity Context**
   - When asking AI agents (Antigravity, Claude, ChatGPT, Gemini) to analyze your daily productivity, debug a system crash, or write daily progress digests, you lack exact timestamped evidence of what applications were open and what system resources were consumed.
3. **Heavy Trackers Drain Battery & RAM**
   - Electron-based time trackers consume hundreds of megabytes of RAM and heavy CPU polling. `sys-chronicle` uses event-driven Unix socket IPC for **0-CPU overhead** window logging.

---

## 💡 How `sys-chronicle` Solves It

- 🪟 **Reliable Hyprland Focus Logging**: Listens directly to Hyprland Wayland Unix socket events (`.socket2.sock`), reconciles the active window every five seconds, and finalizes the active session during a clean shutdown.
- 🔎 **Focused vs. Running Context**: AI exports distinguish focused-window time from applications observed in periodic process samples, so long-running IDEs remain visible when focus events are noisy.
- 📊 **Interactive TUI Dashboard**: Real-time process inspector, fuzzy search (`/`), metric sorting (`s`/`o`), process kill modal (`K`), and screen-time analytics tab (`Tab`).
- 🤖 **1-Keypress AI Clipboard Export (`e`)**: Formats today's activity timeline into Markdown and pipes it straight into your Wayland clipboard (`wl-copy`).
- ⚙️ **Automated Systemd Integration**: Managed as a background user daemon (`sys-chronicle.service`) using ~10 MB RAM.
- 💾 **Lightweight JSONL Storage**: Saves rolling daily logs to `~/.local/share/sys-chronicle/logs/activity-YYYY-MM-DD.jsonl` (~3.6 MB/day).

---

## 🖥️ Interactive TUI Dashboard & Keyboard Shortcuts

Launch the dashboard anytime with:
```bash
sys-chronicle status
```

### Keybindings & Controls

| Shortcut | Action | Description |
| :---: | :--- | :--- |
| **`Tab`** | **Toggle View Tab** | Switch between **Live Dashboard** and **Daily Analytics** (accumulated screen time) |
| **`/`** | **Fuzzy Search** | Filter running applications in real-time as you type |
| **`K`** | **Kill Application** | Opens red confirmation modal to terminate selected app processes (`SIGTERM`) |
| **`e`** | **Instant AI Export** | Formats today's activity into Markdown payload and copies to clipboard (`wl-copy`) |
| **`s`** | **Toggle Sort Metric** | Switch application sorting between **`RAM`** and **`CPU`** load |
| **`o`** | **Toggle Sort Order** | Toggle sorting order (**`Descending ↓`** ↔ **`Ascending ↑`**) |
| **`t`** | **Toggle List Limit** | Cycle application limit count (**`10`** ➔ **`25`** ➔ **`All`**) |
| **`p`** | **Pause / Resume** | Freeze live sampling with relative timestamp badge (`⏸️ PAUSED (X ago)`) |
| **`q`** / **`Esc`** | **Exit TUI** | Return to terminal shell |

---

## 🚀 Installation

### One-liner Quick Install
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

## ⚙️ CLI Usage

```bash
# Launch interactive Ratatui TUI dashboard
sys-chronicle status

# Export activity report for today formatted for AI ingestion
sys-chronicle export

# Copy today's report directly to your clipboard (Wayland)
sys-chronicle export | wl-copy

# Export activity report for a specific date
sys-chronicle export --date 2026-08-10

# Run daemon in foreground (default 5s interval)
sys-chronicle daemon --interval 5

# Install & enable systemd user service
sys-chronicle install-service
```

---

## 🤖 Example Prompt for AI Analysis

Run the export shortcut:
```bash
sys-chronicle export | wl-copy
```

Then paste into your favorite AI prompt:
> *"Here is my sys-chronicle activity log from today. Please analyze my application usage timeline, calculate my active coding vs browsing ratio, and highlight any battery discharge or resource load spikes."*

---

## 📁 Storage & Systemd Service

- **Service Status**: `systemctl --user status sys-chronicle.service`
- **Log Location**: `~/.local/share/sys-chronicle/logs/activity-YYYY-MM-DD.jsonl`

---

## 📄 License

[MIT License](LICENSE)
