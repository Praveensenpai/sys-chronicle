# ⏱️ sys-chronicle — Codebase Architecture & Technical Reference

> **Living Technical Documentation**: This document serves as the high-density technical blueprint and mental model of `sys-chronicle`. Any modification to the codebase (architecture, modules, types, protocols, or storage) **must** be reflected in this file to maintain zero-drift alignment.

---

## 🧭 Repository Overview & Binary Targets

`sys-chronicle` is an ultra-lightweight (~10 MB RAM) background daemon, interactive Ratatui TUI dashboard, and AI context generator built for Arch Linux / Hyprland (Wayland), paired with an anti-cheat MyAnimeList (MAL) companion scrobbler (`sys-chronicle-mal`).

```
                              ┌────────────────────────────────────────┐
                              │  Hyprland Wayland IPC (.socket2.sock)  │
                              └──────────────────┬─────────────────────┘
                                                 │ event-driven focus
                                                 ▼
┌──────────────────────┐              ┌──────────────────────┐              ┌───────────────────────────┐
│ Linux /sys Subsystem │ ──power/hw──▶│ sys-chronicle Daemon │◀──JSON IPC───│      MPV Media Player     │
│ (battery, hwmon)     │              │    (src/main.rs)     │              │     (/tmp/mpvsocket)      │
└──────────────────────┘              └──────────┬───────────┘              └───────────────────────────┘
                                                 │
                                                 │ writes daily JSONL
                                                 ▼
                              ┌──────────────────────────────────────┐
                              │ ~/.local/share/sys-chronicle/logs/   │
                              │   activity-YYYY-MM-DD.jsonl          │
                              └──────────┬───────────────────┬───────┘
                                         │                   │
                     ┌───────────────────┘                   └──────────────────┐
                     ▼                                                          ▼
       ┌───────────────────────────┐                              ┌───────────────────────────┐
       │   Interactive Ratatui     │                              │   Plugin Dispatch Engine  │
       │      TUI Dashboard        │                              │ (stdin JSON pipe to hooks)│
       │   (sys-chronicle status)  │                              └─────────────┬─────────────┘
       └───────────────────────────┘                                            │
                                                                                ▼
                                                                  ┌───────────────────────────┐
                                                                  │     sys-chronicle-mal     │
                                                                  │   (Companion Scrobbler)   │
                                                                  └─────────────┬─────────────┘
                                                                                │
                                              ┌─────────────────────────────────┴───────────────────────────────┐
                                              ▼                                                                 ▼
                               ┌─────────────────────────────┐                                   ┌─────────────────────────────┐
                               │     Gemini 3.5 Flash AI     │                                   │     MyAnimeList API v2      │
                               │ Classifier & Episode Parser │                                   │ (OAuth2 PKCE Scrobbler)     │
                               └─────────────────────────────┘                                   └─────────────────────────────┘
```

### Binary Targets (`Cargo.toml`)
1. **`sys-chronicle`** (`src/main.rs`): Main executable. Runs background telemetry daemon, launches Ratatui TUI, generates AI export prompts, and installs the user systemd service.
2. **`sys-chronicle-mal`** (`src/bin/mal.rs`): Official companion binary. Handles MAL OAuth2 PKCE login, status checking, filename parsing tests, manual sync, and runs as an event-driven plugin hook receiving JSON on `stdin`.
3. **`sys_chronicle`** (`src/lib.rs`): Shared internal library exposing modules to both binaries and test suites.

---

## 📁 Source File Tree & Responsibilities

| Path | Primary Structs / Functions | Responsibilities & Logic |
| :--- | :--- | :--- |
| `src/main.rs` | `Cli`, `Commands`, `main()` | CLI parsing (`clap`), initializes Tokiomultitask daemon (`Window`, `Power`, `Metrics`, `Mpv`), dispatches `status`, `summary`, `export`, and `install-service`. |
| `src/lib.rs` | Public module exports | Re-exports `exporter`, `logger`, `mal`, `monitor`, `plugin`, `service`, `tui`. |
| `src/bin/mal.rs` | `Cli`, `Commands`, `run_login()`, `run_status()`, `run_sync()` | Companion binary CLI. Handles OAuth PKCE setup, status check, test parsing, manual/forced MAL sync, and plugin hook stdin pipe. |
| **`src/logger/`** | | |
| `src/logger/mod.rs` | `ActivityEvent`, `LogWriter` | Module definitions and re-exports. |
| `src/logger/event.rs` | `ActivityEvent` (enum) | Tagged serde JSON event definitions: `WindowFocus`, `PowerState`, `SystemMetrics`, `MediaPlayback`, `MediaWatchThreshold`. |
| `src/logger/writer.rs` | `LogWriter` | Rolling daily JSONL file storage (`~/.local/share/sys-chronicle/logs/activity-YYYY-MM-DD.jsonl`). Read and query functions by date and date ranges. |
| **`src/monitor/`** | | |
| `src/monitor/mod.rs` | Re-exports | Exports monitors, `PlaybackInterval`, and tracker types. |
| `src/monitor/window.rs` | `WindowMonitor`, `HyprActiveWindow` | Connects to Hyprland IPC `.socket2.sock`. Listens for `activewindow>>` and `windowtitle>>`. Computes dwell duration on switch. Falls back to `hyprctl activewindow -j` polling if socket drops. |
| `src/monitor/power.rs` | `PowerMonitor`, `PowerState` | Inspects `/sys/class/power_supply` for `BAT*` (capacity, status) and `AC*`/`ADP*` (online). Emits events on state shift, $\ge 2\%$ change, or 5-min heartbeat. |
| `src/monitor/metrics.rs` | `MetricsMonitor`, `MetricsSnapshot`, `AppDetail` | `sysinfo` process sampler. Deduplicates kernel/worker threads via `/proc/{pid}/status` (`Pid == Tgid`) and tracks physical VmRSS memory. Reads `/sys/class/hwmon` for CPU package temp and fan RPM. |
| `src/monitor/mpv.rs` | `MpvMonitor`, `MpvEvent` | Connects to MPV Unix domain socket (`/tmp/mpvsocket`, `$XDG_RUNTIME_DIR/mpvsocket`). Observes properties (`media-title`, `path`, `pause`, `time-pos`, `duration`). Dispatches events to writer and plugins. |
| `src/monitor/mpv_session.rs`| `MpvPlaybackSession`, `MpvProgressParams` | Maintains active MPV session state machine. Detects title/path transitions, pause/resume, seek forward/backward ($>5$s), updates `UniqueTimelineTracker`, debounces sync to `AnimeSyncHistory` every 3s. |
| `src/monitor/timeline_tracker.rs` | `UniqueTimelineTracker`, `PlaybackInterval` | **Anti-cheat watch calculation**. Merges disjoint playback intervals `[start..end]`. Calculates `total_unique_secs / duration`. Triggers 80% threshold exactly once. Rejects scrubbing and looping exploits. |
| **`src/mal/`** | | |
| `src/mal.rs` | Module re-exports | Exposes all MAL client, auth, parser, and history interfaces. |
| `src/mal/auth.rs` | `MalAuth`, `MalConfig`, `TokenResponse` | MAL OAuth2 PKCE flow. Spins up temporary local server on `127.0.0.1:8080/callback`. Auto-refreshes tokens expiring within 5 min. Config stored in `~/.config/sys-chronicle/mal_config.json`. |
| `src/mal/candidate.rs` | `select_best_candidate()`, `score_candidate()` | Scored candidate matching against MAL search results. Awards points for title match (+150), season match (+100), user list presence (+50); heavily penalizes mismatched seasons (-200), movies, and specials (-200). |
| `src/mal/classifier.rs` | `AnimeClassifier`, `MediaClassification` | Filters non-syncable content (bonus extras, creditless OP/EDs, live-action footage, trailers, disc menus) using directory heuristics and Gemini 3.5 Flash AI classifier. |
| `src/mal/client.rs` | `MalClient`, `MalAnimeNode`, `UserProfile`, `UserListStatus` | Reqwest HTTP client for MyAnimeList API v2 (`/users/@me`, `/anime?q=...`, `/anime/{id}/my_list_status`). Sets status to `watching` or `completed`. |
| `src/mal/gemini.rs` | `GeminiParser`, `GeminiClassification` | Google Gemini 3.5 Flash REST client. Uses structured output (`responseSchema`) to extract canonical titles, episode numbers, and non-syncable classifications. |
| `src/mal/handler.rs` | `MalHandler` | Central scrobbler coordinator. Handles stdin JSON hook, 80% threshold events, classification, candidate search, "Ahead-of-MAL" downgrade guard, and `notify-send` desktop alerts. |
| `src/mal/history.rs` | `AnimeSyncHistory`, `AnimeSyncRecord`, `SyncStatus` | Local persistent state in `~/.local/share/sys-chronicle/anime_sync_history.json`. Tracks playback intervals, position, watch %, and MAL status badges. |
| `src/mal/parser.rs` | `AnimeParser`, `AnimeInfo` | Offline regex title/episode/season extractor. Strips release group tags (`[SubsPlease]`), hashes, resolutions (`1080p`), and codecs. Handles diverse anime file naming conventions. |
| **`src/tui/`** | | |
| `src/tui.rs` | `run_status_tui()`, `App` | Full Ratatui TUI dashboard. 3 tabs, state management, search (`/`), sort (`s`/`o`), limit (`t`), pause (`p`), AI export modal (`e`), process kill modal (`K`). |
| `src/tui/media_tab.rs` | `render_media_tab()`, `MediaTabState` | Tab 3 layout: anime list pane (left) + media inspector pane (right), force sync modal confirmation. |
| `src/tui/media_inspector.rs`| `render_anime_inspector()`, `render_force_sync_modal()` | Detailed inspector view: metadata, dual progress bars (Seek Head vs Anti-Cheat Unique Watched), status badge explanation. |
| **`src/exporter/`** | | |
| `src/exporter/mod.rs` | Re-exports | Re-exports `generate_ai_report`. |
| `src/exporter/ai.rs` | `generate_ai_report()` | Formats daily JSONL activity logs into a Markdown prompt with app usage, timeline intervals, brief focus buckets (<5s), battery discharge deltas, and CPU/RAM load. |
| `src/plugin.rs` | `PluginDispatcher` | Plugin discovery and execution engine. Reads executables in `~/.config/sys-chronicle/plugins/`, pipes JSON events over `stdin` asynchronously with a 30s timeout. |
| `src/service.rs` | `install_user_service()` | Generates `sys-chronicle.service` in `~/.config/systemd/user/` and enables/starts it via `systemctl --user`. |
| `install.sh` | Shell installer script | Builds or downloads binary, copies to `~/.local/bin/`, symlinks companion plugin, and starts systemd service. |

---

## 💾 Storage Layout & Configuration Files

| Purpose | Path | Format |
| :--- | :--- | :--- |
| **Activity Logs** | `~/.local/share/sys-chronicle/logs/activity-YYYY-MM-DD.jsonl` | Rolling JSON Lines |
| **MAL Sync State** | `~/.local/share/sys-chronicle/anime_sync_history.json` | JSON Array of `AnimeSyncRecord` |
| **MAL OAuth & AI Config** | `~/.config/sys-chronicle/mal_config.json` | JSON Object (`client_id`, tokens, keys) |
| **Plugin Directory** | `~/.config/sys-chronicle/plugins/` | Executable files / symlinks |
| **Systemd User Unit** | `~/.config/systemd/user/sys-chronicle.service` | Systemd service unit file |

---

## 📡 IPC Protocols & Socket Contracts

### 1. Hyprland Wayland IPC (`WindowMonitor`)
- **Socket Discovery**: Searches `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock`. If environment variable is absent, scans `$XDG_RUNTIME_DIR/hypr/` for directories containing `.socket2.sock`.
- **Events Read**:
  - `activewindow>>[class],[title]`: Emitted on active window switch. Dwell duration of previous window is calculated and logged.
  - `windowtitle>>[title]`: Emitted when focused window title updates (e.g., browser tab change).
- **Fallback**: If socket is unavailable or disconnects, polls `hyprctl activewindow -j` every 2 seconds.

### 2. MPV JSON IPC (`MpvMonitor`)
- **Socket Discovery**: Probes `/tmp/mpvsocket`, `$XDG_RUNTIME_DIR/mpvsocket`, `$XDG_RUNTIME_DIR/mpv.sock`, and `~/.config/mpv/socket`.
- **Initialization Command**:
  ```json
  {"command": ["observe_property", 1, "media-title"]}
  {"command": ["observe_property", 2, "path"]}
  {"command": ["observe_property", 3, "pause"]}
  {"command": ["observe_property", 4, "time-pos"]}
  {"command": ["observe_property", 5, "duration"]}
  ```
- **Events Dispatched**:
  - `ActivityEvent::MediaPlayback`: On start, stop, pause, resume, seek_forward, seek_backward.
  - `ActivityEvent::MediaWatchThreshold`: Emitted once when unique timeline coverage reaches $\ge 80\%$.

### 3. Plugin Dispatch Protocol (`PluginDispatcher`)
- **Location**: `~/.config/sys-chronicle/plugins/` (scans all files with executable permissions `0o111`).
- **Contract**: Daemon serializes `ActivityEvent` to JSON and pipes it directly into the plugin process's `stdin`. Output is ignored; errors on `stderr` are logged. Execution is governed by a 30-second timeout.

### 4. MyAnimeList OAuth2 PKCE Callback (`MalAuth`)
- **Endpoint**: Local HTTP listener bound temporarily to `127.0.0.1:8080`.
- **Redirect URI**: `http://localhost:8080/callback`.
- **PKCE**: 128-char random alphanumeric verifier with plain code challenge method. Automatically exchanges auth code for access and refresh tokens.

---

## 📜 Log Schema: `ActivityEvent` (JSONL)

All events written to `activity-YYYY-MM-DD.jsonl` are tagged with `"type"`:

### 1. `window_focus`
```json
{
  "type": "window_focus",
  "timestamp": "2026-09-13T18:00:00.123+05:30",
  "app_class": "kitty",
  "title": "nvim src/main.rs",
  "duration_secs": 42
}
```
*(Note: `duration_secs` is `null` on entry and populated when the window loses focus.)*

### 2. `power_state`
```json
{
  "type": "power_state",
  "timestamp": "2026-09-13T18:00:00.123+05:30",
  "status": "Discharging",
  "capacity": 85,
  "ac_online": false
}
```

### 3. `system_metrics`
```json
{
  "type": "system_metrics",
  "timestamp": "2026-09-13T18:00:00.123+05:30",
  "cpu_pct": 14.2,
  "ram_used_mb": 4096,
  "ram_total_mb": 32000,
  "ram_pct": 12.8,
  "top_apps": ["firefox (1200 MB)", "kitty (350 MB)"]
}
```

### 4. `media_playback`
```json
{
  "type": "media_playback",
  "timestamp": "2026-09-13T18:00:00.123+05:30",
  "player": "mpv",
  "event_type": "start",
  "title": "Sousou no Frieren - 04",
  "path": "/home/user/Videos/Sousou no Frieren - 04.mkv",
  "position_secs": 0,
  "duration_secs": 1440
}
```

### 5. `media_watch_threshold`
```json
{
  "type": "media_watch_threshold",
  "timestamp": "2026-09-13T18:19:13.123+05:30",
  "player": "mpv",
  "title": "Sousou no Frieren - 04",
  "path": "/home/user/Videos/Sousou no Frieren - 04.mkv",
  "duration_secs": 1440,
  "watched_secs": 1152,
  "watch_pct": 80.0
}
```

---

## 🧮 Algorithms & Core Logic

### 1. Anti-Cheat Unique Playback Coverage (`UniqueTimelineTracker`)
- **Position Tracking**: When `time-pos` increments by $\le 3$s in unpaused state, an interval `[prev_pos..current_pos]` is recorded.
- **Interval Merging (`merge_intervals`)**:
  - Sorts intervals by `start`.
  - For adjacent or overlapping intervals where `interval.start <= last.end`, updates `last.end = max(last.end, interval.end)`.
- **Metrics**:
  - `total_unique_secs = sum(end - start) for all merged intervals`.
  - `coverage_pct = (total_unique_secs / duration_secs) * 100.0` (clamped to 100%).
- **Threshold**: When `coverage_pct >= 80.0%`, threshold event fires exactly once per media key.

### 2. Candidate Matching Score (`candidate.rs`)
Given query title $Q$, candidate $C$, and target season $S$:
- **Base Match**: Normalized alphanumeric comparison without punctuation or case.
  - Full match: `+150`
  - Prefix match: `+80`
  - Substring match: `+40`
- **Media Type Modifiers**:
  - If query is normal TV series but candidate is Movie or Special: `-200` penalty.
  - If query is Movie and candidate is Movie: `+120`.
  - If candidate season == target season: `+100` (mismatched: `-200`).
- **User List Bonus**:
  - In user's list: `+50` (`+20` if `num_episodes_watched > 0`).
- **Length Penalty**: `-min(30, abs(len(C) - len(Q)))`.

### 3. Ahead-of-MAL Downgrade Guard (`handler.rs`)
- Compares `info.episode` against user's current MAL watched episode count.
- If `mal_current_ep > info.episode` and `force == false`:
  - **Does NOT** call MAL API update.
  - Flags record as `SyncStatus::AheadOnMal { mal_episode }`.
  - Sends desktop notification informing user that progress was preserved.
  - Can only be overridden with `sys-chronicle-mal sync <NAME> --force` or pressing `f` in the TUI confirmation modal.

---

## ⌨️ TUI Architecture & Keymaps

The TUI operates on a 250ms tick loop rendering via `ratatui` with `CrosstermBackend`.

### Views
1. **Live Dashboard (`ActiveTab::Live`)**:
   - Gauges: Global CPU load, RAM utilization.
   - Hardware: CPU package temperature and fan RPM scraped from `/sys/class/hwmon`.
   - Active Window: Focused window class and title.
   - Interactive Process Table: Deduplicated RSS memory and CPU usage per executable.
2. **Daily Analytics (`ActiveTab::Analytics`)**:
   - Aggregated screen time per application for the current day computed from JSONL focus intervals.
3. **Anime & MAL Chronicle (`ActiveTab::MediaSync`)**:
   - Left: Scrollable anime playback history with status badges (`✔ SYNCED`, `⏩ MAL AHEAD`, `▶ WATCHING`, `🎬 LIVE-ACT`).
   - Right: Media inspector with seek head gauge vs anti-cheat unique watched gauge, episode details, and action shortcuts.

### Global & Contextual Keybindings
| Key | Context | Action |
| :---: | :--- | :--- |
| **`Tab`** | Global | Cycle views (`Live` ➔ `Analytics` ➔ `MediaSync`) |
| **`q`** / **`Esc`** | Global | Close modal or exit TUI |
| **`/`** | Live / Analytics | Enter fuzzy search filter mode |
| **`K`** | Live | Open red confirmation modal to terminate selected app (`SIGTERM`) |
| **`e`** | Global | Open AI export modal (Today, Yesterday, Last 7d, Last 30d, Date, Range) |
| **`s`** / **`Enter`** | Live | Toggle sort metric between RAM and CPU |
| **`s`** / **`Enter`** | MediaSync | Trigger manual background MAL sync for selected anime |
| **`f`** | MediaSync | Open force overwrite confirmation modal (overrides `AheadOnMal`) |
| **`r`** | MediaSync | Reload playback history from disk |
| **`o`** | Live / Analytics | Toggle sort direction (`Descending ↓` ↔ `Ascending ↑`) |
| **`t`** | Live / Analytics | Cycle process list limit (`10` ➔ `25` ➔ `All`) |
| **`p`** | Live | Pause / resume live metric sampling with frozen timestamp badge |

---

## 🛠️ CLI Quick Reference

```bash
# Daemon
sys-chronicle daemon --interval 5          # Start daemon (default 5s interval)
sys-chronicle install-service               # Setup and enable systemd user unit

# TUI & Status
sys-chronicle status                        # Launch Ratatui TUI dashboard
sys-chronicle status --plain                # Print plain-text snapshot

# Export & Summary
sys-chronicle export                        # Export today's activity as AI prompt
sys-chronicle export --date 2026-09-10      # Export specific date
sys-chronicle export --days 7               # Export last 7 days
sys-chronicle export | wl-copy              # Pipe directly to Wayland clipboard

# MyAnimeList Scrobbler (sys-chronicle-mal)
sys-chronicle-mal login                     # Interactive OAuth2 PKCE login
sys-chronicle-mal status                    # Check MAL connection & token health
sys-chronicle-mal test "[Group] Title - 01" # Test regex/Gemini title parsing
sys-chronicle-mal sync "Anime Title"        # Manually sync an anime
sys-chronicle-mal sync "Anime Title" --force# Force overwrite MAL progress
sys-chronicle-mal config --toast 2          # Set notification duration to 2s
sys-chronicle-mal hook                      # Run in stdin event hook mode
```

---

## 🔄 Maintenance & Sync Protocol

Whenever making code or structural changes to this repository:
1. **New Modules / Structs**: Add entries to the *Source File Tree & Responsibilities* table.
2. **Schema Modifications**: Update the *Log Schema: `ActivityEvent`* section with new fields or variants.
3. **Socket or IPC Changes**: Document new commands or socket endpoints under *IPC Protocols & Socket Contracts*.
4. **Keybindings**: Update the *Global & Contextual Keybindings* matrix if shortcuts change.
5. **Zero Dead Code & Warning Suppression**: Adhere strictly to clean code standards (no `#[allow(dead_code)]`, no unwrap/expect in production code paths, `<400` lines/file soft limit).
