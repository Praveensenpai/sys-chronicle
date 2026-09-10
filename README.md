<div align="center">

# ⏱️ sys-chronicle
### システム・クロニクル — High-Performance System Activity Logger & Interactive TUI Explorer

> **Ultra-lightweight Wayland window focus tracker, Ratatui dashboard, and AI context generator for Arch Linux.**

[![Latest Release](https://img.shields.io/github/v/release/Praveensenpai/sys-chronicle?style=for-the-badge&color=89b4fa)](https://github.com/Praveensenpai/sys-chronicle/releases)
[![Rust 2024](https://img.shields.io/badge/Rust-2024%20Edition-DEA584?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Arch%20Linux-1793d1?style=for-the-badge&logo=archlinux&logoColor=white)](https://archlinux.org)
[![Wayland](https://img.shields.io/badge/Compositor-Wayland%20%7C%20Hyprland-00a86b?style=for-the-badge&logo=wayland&logoColor=white)](https://hyprland.org)
[![License](https://img.shields.io/badge/License-MIT-a6e3a1?style=for-the-badge)](LICENSE)

<br>

[📸 Visual Showcase](#-visual-showcase) • [⚡ Quick Start](#-quick-start) • [✨ Features](#-key-features) • [⌨️ Keybindings](#%EF%B8%8F-keyboard-shortcuts--controls) • [💻 CLI Usage](#-cli-usage) • [🎬 Anime & MAL Sync](#-myanimelist-companion-scrobbler-sys-chronicle-mal)

</div>

---

`sys-chronicle` is an ultra-lightweight (~10 MB RAM) Rust background daemon and feature-packed Ratatui TUI dashboard. It silently tracks active window focus, system power states, CPU/RAM utilization, CPU temperature, fan speed, and process metrics into daily JSON Lines logs, formatted into instant AI-digestible Markdown reports.

> [!TIP]
> **Zero CPU Polling Overhead · ~10 MB RAM · Instant AI Clipboard Export (`e`)**
> Listens directly to Hyprland's Wayland IPC socket (`.socket2.sock`) with event-driven zero polling overhead. Generates complete productivity timelines ready to pipe directly into your favorite LLM.

---

## 📸 Visual Showcase

<div align="center">

### ⚡ Tab 1: Real-Time Live System Dashboard
*Live CPU & RAM utilization gauges, hardware sensor telemetry (Package Temp & Fan RPM), active Wayland window focus, and interactive process inspector.*

![Live Dashboard](assets/tui_live_dashboard.png)

<br>

### 📊 Tab 2: Daily Accumulated Screen Time Analytics
*Automated breakdown of active focused window time per application across the entire day.*

![Daily Analytics](assets/tui_daily_analytics.png)

<br>

### 🎬 Tab 3: Anime Chronicle & MyAnimeList Companion Inspector
*Anti-cheat 80% unique coverage timeline gauge, seek-head vs. watched progress tracking, and automated MAL synchronization status.*

![Anime & MAL Sync](assets/tui_anime_mal_sync.png)

</div>

<details>
<summary><b>🧠 View Gemini 3.5 Flash AI Classifier & Non-Anime Extra Detection (Click to expand)</b></summary>
<br>

*Intelligent identification of live-action events, voice actress specials, Blu-ray extras, and audio dramas—bypassing MAL sync without polluting watch counts.*

<div align="center">

![Anime AI Classifier](assets/tui_anime_ai_classifier.png)

</div>

</details>

---

## 🚀 Quick Start

### 🪄 One-Liner Magic (Recommended)

Install `sys-chronicle` and its companion scrobbler in seconds:

```bash
curl -sSL https://raw.githubusercontent.com/Praveensenpai/sys-chronicle/main/install.sh | bash
```

### 📦 Build From Source

```bash
git clone https://github.com/Praveensenpai/sys-chronicle.git
cd sys-chronicle
cargo build --release
cp target/release/sys-chronicle ~/.local/bin/
cp target/release/sys-chronicle-mal ~/.local/bin/
```

---

## ✨ Key Features

| Feature | Description |
| :--- | :--- |
| 🪟 **Event-Driven Focus Logging** | Connects to Hyprland Wayland IPC (`.socket2.sock`) for true **0% CPU polling** window focus capture. |
| 📊 **Interactive Ratatui TUI** | 3 full-featured views: Live Telemetry, Daily Screen Time Analytics, and Anime Chronicle Inspector. |
| 🎬 **Anti-Cheat 80% Scrobbler** | Merges disjoint intervals to calculate true unique playback coverage—no scrubbing exploits. |
| 🧠 **Gemini 3.5 Flash AI Classifier** | Distinguishes canonical anime episodes from live-action events, Blu-ray extras, and bonuses. |
| 🛡️ **Ahead-of-MAL Guard** | Prevents re-watching older episodes from accidentally downgrading progress already logged on MAL. |
| 🤖 **1-Keypress AI Export (`e`)** | Formats today's complete timeline into Markdown and copies straight to clipboard (`wl-copy`). |
| ⚙️ **Lightweight Systemd Daemon** | Operates silently as a background user daemon (`sys-chronicle.service`) consuming ~10 MB RAM. |
| 💾 **Rolling JSONL Storage** | Saves daily audit trails to `~/.local/share/sys-chronicle/logs/activity-YYYY-MM-DD.jsonl`. |

---

## ⌨️ Keyboard Shortcuts & Controls

Launch the interactive dashboard anytime with:

```bash
sys-chronicle status
```

| Shortcut | Action | Description |
| :---: | :--- | :--- |
| **`Tab`** | **Cycle Views** | Cycle across **Live Dashboard** ➔ **Daily Analytics** ➔ **Anime & MAL Sync** |
| **`/`** | **Fuzzy Search** | Filter running applications in real-time as you type *(Live/Analytics)* |
| **`K`** | **Kill Application** | Opens red confirmation modal to terminate selected app processes (`SIGTERM`) |
| **`e`** | **Instant AI Export** | Formats today's activity into Markdown payload and copies to clipboard (`wl-copy`) |
| **`s`** / **`Enter`** | **Sync Anime / Sort** | On Anime tab: manual sync to MAL. On Live tab: toggle RAM/CPU sort |
| **`f`** | **Force Sync Anime** | On Anime tab: opens confirmation modal to force overwrite MAL when MAL is ahead |
| **`r`** | **Refresh Anime** | On Anime tab: reloads playback history from disk |
| **`o`** | **Toggle Sort Order** | Toggle sorting order (**`Descending ↓`** ↔ **`Ascending ↑`**) |
| **`t`** | **Toggle List Limit** | Cycle application limit count (**`10`** ➔ **`25`** ➔ **`All`**) |
| **`p`** | **Pause / Resume** | Freeze live sampling with relative timestamp badge (`⏸️ PAUSED (X ago)`) |
| **`q`** / **`Esc`** | **Exit TUI** | Return to terminal shell |

---

## 💻 CLI Usage

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

## 🎬 MyAnimeList Companion Scrobbler (`sys-chronicle-mal`)

`sys-chronicle-mal` is the official companion binary that scrobbles watched anime to MyAnimeList when reaching the real 80% watch threshold in MPV.

### Accurate 80% Watch Threshold Detection

Unlike naive scrobblers that merely check `time-pos / duration >= 0.8` (which triggers if you scrub to the credits) or count wall-clock seconds (which inflates if you loop a 2-minute scene), `sys-chronicle` uses **Unique Timeline Coverage**:

```text
Playback Timeline:
[00:00 ─── Episode Start ─── 05:00]  ✔ Watched (5 mins)
               [08:00 ─────── 18:00]  ✔ Watched (10 mins)
                              [20:00 ─── 24:00]  ✔ Watched (4 mins)
───────────────────────────────────────────────────────────────────
Total Unique Watched: 19 mins / 24 mins = 79.1% (Pending...)
+ Continuous Watch to 19:13 = 80.0% ──➔ 🚀 [✔ SYNCED to MyAnimeList]
```

- Continuously merges disjoint playback intervals `[start..end]`.
- Calculates `unique_watched_seconds / video_duration >= 0.80`.
- Once 80% real coverage is reached, triggers MAL synchronization without spamming.

### Scrobbler Features
- 🔑 **Official MAL API v2 with PKCE OAuth2**: Secure interactive browser login with zero secret leaking.
- 🧠 **Hybrid Gemini Flash-Lite AI + Regex Parser**: Uses Google's Gemini Flash (`gemini-3.5-flash`) to parse convoluted release group titles (`[SubsPlease] Sousou no Frieren - 04 (1080p).mkv` ➔ Canonical Title: *"Sousou no Frieren"*, Ep `4`). Automatically falls back to an offline regex parser if a Gemini API key is not configured.
- 📊 **Dedicated 3rd TUI Tab**: Switch to the **Anime & MAL Sync** tab (<kbd>Tab</kbd>) to inspect playback progress bars, watch percentages, and sync statuses (`[▶ WATCHING]`, `[✓ SYNCED]`, `[⏩ MAL AHEAD]`, `[🎬 LIVE-ACT]`).
- 🛡️ **Ahead-of-MAL Downgrade Guard**: Prevents re-watching earlier episodes from overwriting higher progress already recorded on MyAnimeList.
- ⚡ **Manual & Force Sync**: Instant sync inside the TUI (<kbd>s</kbd> / <kbd>Enter</kbd>) or via CLI (`sys-chronicle-mal sync <NAME>`), with modal confirmation (<kbd>f</kbd> or `--force`) to override MAL progress when intentional.
- 🔔 **Desktop Notifications**: Sends desktop alerts via `notify-send` when an episode is updated.

### Setup & Commands

```bash
# 1. Connect your MyAnimeList account (and optionally provide Gemini API Key)
sys-chronicle-mal login

# 2. Check connection health and profile status
sys-chronicle-mal status

# 3. Test anime parsing on any video filename
sys-chronicle-mal test "[SubsPlease] Sousou no Frieren - 04 (1080p) [9A1B2C3D].mkv"

# 4. Manually sync an anime title (with optional --force to overwrite MAL)
sys-chronicle-mal sync "Sousou no Frieren"
sys-chronicle-mal sync "Sousou no Frieren" --force

# 5. Link as an active plugin for sys-chronicle (handled automatically by install.sh)
mkdir -p ~/.config/sys-chronicle/plugins
ln -sf ~/.local/bin/sys-chronicle-mal ~/.config/sys-chronicle/plugins/sys-chronicle-mal
```

---

## 🔌 Event-Driven Plugin System

`sys-chronicle` features an asynchronous, non-blocking plugin hook engine. Any executable file placed inside `~/.config/sys-chronicle/plugins/` will automatically receive system activity events formatted as JSON over `stdin`.

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
