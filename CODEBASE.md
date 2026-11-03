# CODEBASE.md: sys-chronicle Semantic Digest

> **Notice**: AI-optimized semantic index. Do not write narrative prose. Keep token density high.

## 1. System Topology & Data Flow
```text
Entrypoint ──> CLI/Parser ──> Domain Logic ──> Infra/IO
```

## 2. Global Constraints & Architecture Patterns
- **Primary Language**: Rust 2021 edition
- **Architectural Paradigm**: Role-based (domain/, infra/, api/cli/, tui/)
- **Hard Constraints**: <400 lines/file, <60 lines/fn, zero production unwrap(), 0 warnings.
- **Target Distribution**: Linux x86_64 standalone binary

## 3. Module & Interface Skeleton

### `src/bin/mal.rs` (Role: general, Lines: 392)
- **Responsibility**: Core general logic in src/bin/mal.rs
- **Imports**: use anyhow :: { Context , Result } , use chrono :: { DateTime , Local , TimeZone } , use clap :: { Parser , Subcommand } , use std :: env , use std :: io :: { self , IsTerminal , Write } , use sys_chronicle :: mal :: { AnimeParser , GeminiParser , MalAuth , MalClient , MalHandler } 

### `src/exporter/ai.rs` (Role: general, Lines: 455)
- **Responsibility**: Core general logic in src/exporter/ai.rs
- **Imports**: use std :: collections :: BTreeMap , use crate :: logger :: ActivityEvent 
- **Public Functions & Signatures**:
  ```rust
  fn generate_ai_report (events : & [ActivityEvent] , title_date : & str) -> String
  ```

### `src/exporter/mod.rs` (Role: general, Lines: 3)
- **Responsibility**: Core general logic in src/exporter/mod.rs
- **Imports**: pub use ai :: generate_ai_report 

### `src/lib.rs` (Role: general, Lines: 7)
- **Responsibility**: Core general logic in src/lib.rs

### `src/logger/event.rs` (Role: general, Lines: 64)
- **Responsibility**: Core general logic in src/logger/event.rs
- **Imports**: use serde :: { Deserialize , Serialize } 
- **Types & Enums**:
  ```rust
  pub enum ActivityEvent
  ```
- **Public Functions & Signatures**:
  ```rust
  fn timestamp (& self) -> & str
  ```

### `src/logger/mod.rs` (Role: general, Lines: 5)
- **Responsibility**: Core general logic in src/logger/mod.rs
- **Imports**: pub use event :: ActivityEvent , pub use writer :: LogWriter 

### `src/logger/writer.rs` (Role: general, Lines: 110)
- **Responsibility**: Core general logic in src/logger/writer.rs
- **Imports**: use anyhow :: { Context , Result } , use chrono :: Local , use std :: fs :: { create_dir_all , OpenOptions } , use std :: io :: Write , use std :: path :: PathBuf , use super :: event :: ActivityEvent 
- **Types & Enums**:
  ```rust
  pub struct LogWriter
  ```
- **Public Functions & Signatures**:
  ```rust
  fn new () -> Result < Self >
  fn get_logs_dir () -> PathBuf
  fn write_event (& self , event : & ActivityEvent) -> Result < () >
  fn read_events_for_date (date_str : & str) -> Result < Vec < ActivityEvent > >
  fn read_recent_events (days : usize) -> Result < Vec < ActivityEvent > >
  fn read_events_for_date_range (start_date : & str , end_date : & str ,) -> Result < Vec < ActivityEvent > >
  ```

### `src/main.rs` (Role: general, Lines: 221)
- **Responsibility**: Core general logic in src/main.rs
- **Imports**: use anyhow :: Result , use chrono :: Local , use clap :: { ArgAction , Parser , Subcommand } , use std :: io :: IsTerminal , use std :: sync :: atomic :: { AtomicBool , Ordering } , use std :: sync :: Arc , use sysinfo :: System , use tokio :: signal , use exporter :: generate_ai_report , use logger :: LogWriter , use monitor :: { MetricsMonitor , MpvMonitor , PowerMonitor , WindowMonitor } 

### `src/mal/auth.rs` (Role: general, Lines: 312)
- **Responsibility**: Core general logic in src/mal/auth.rs
- **Imports**: use anyhow :: { Context , Result } , use chrono :: { Duration as ChronoDuration , Local } , use rand :: { distributions :: Alphanumeric , Rng } , use reqwest :: Client , use serde :: { Deserialize , Serialize } , use std :: fs , use std :: path :: PathBuf , use std :: process :: Command , use tokio :: io :: { AsyncReadExt , AsyncWriteExt } , use tokio :: net :: TcpListener , use url :: Url 
- **Types & Enums**:
  ```rust
  pub struct MalConfig
  pub struct MalAuth
  ```
- **Public Functions & Signatures**:
  ```rust
  fn is_deepseek_enabled (& self) -> bool
  fn get_deepseek_url (& self) -> String
  fn get_deepseek_model (& self) -> String
  fn get_deepseek_key (& self) -> String
  fn get_gemini_model (& self) -> String
  fn config_path () -> PathBuf
  fn load_config () -> Result < MalConfig >
  fn save_config (config : & MalConfig) -> Result < () >
  fn generate_code_verifier () -> String
  async fn login (client_id : String , client_secret : Option < String > , gemini_api_key : Option < String > ,) -> Result < () >
  async fn get_valid_token (config : & mut MalConfig) -> Result < String >
  ```

### `src/mal/candidate.rs` (Role: general, Lines: 488)
- **Responsibility**: Core general logic in src/mal/candidate.rs
- **Imports**: use super :: client :: { MalAnimeNode , MediaKind } , use regex :: Regex 
- **Types & Enums**:
  ```rust
  pub struct ScoredCandidate
  ```
- **Public Functions & Signatures**:
  ```rust
  fn normalize_for_comparison (s : & str) -> String
  fn detect_season_and_type (title : & str) -> (Option < u32 > , bool , bool)
  fn strip_season_and_type_tags (normalized : & str) -> String
  fn seed_query_from_title (title : & str) -> String
  fn season_from_path (media_path : & str) -> Option < u32 >
  fn score_candidate (candidate : & MalAnimeNode , query_title : & str , target_season : Option < u32 > , query_kind : MediaKind ,) -> Option < i32 >
  fn build_search_queries (title : & str , season : Option < u32 >) -> Vec < String >
  fn select_best_candidate < 'a > (candidates : & 'a [MalAnimeNode] , query_title : & str , target_season : Option < u32 > ,) -> Option < & 'a MalAnimeNode >
  ```

### `src/mal/classifier.rs` (Role: general, Lines: 201)
- **Responsibility**: Core general logic in src/mal/classifier.rs
- **Imports**: use anyhow :: Result , use regex :: Regex , use std :: path :: Path , use super :: gemini :: GeminiParser , use super :: parser :: AnimeInfo 
- **Types & Enums**:
  ```rust
  pub enum MediaClassification
  pub struct AnimeClassifier
  ```
- **Public Functions & Signatures**:
  ```rust
  fn check_local_heuristics (raw_path_or_title : & str) -> Option < MediaClassification >
  async fn classify (raw_path_or_title : & str , gemini : Option < & GeminiParser > ,) -> Result < Option < MediaClassification > >
  ```

### `src/mal/client.rs` (Role: cli, Lines: 205)
- **Responsibility**: Core cli logic in src/mal/client.rs
- **Imports**: use anyhow :: { Context , Result } , use reqwest :: { header , Client } , use serde :: { Deserialize , Serialize } 
- **Types & Enums**:
  ```rust
  pub struct MalAnimeNode
  pub struct AlternativeTitles
  pub enum MediaKind
  pub struct MalAnimeData
  pub struct MalSearchResponse
  pub struct UserListStatus
  pub struct UserProfile
  pub struct MalClient
  ```
- **Public Functions & Signatures**:
  ```rust
  fn all_titles (& self) -> Vec < & str >
  fn media_kind (& self) -> MediaKind
  fn new (token : String) -> Result < Self >
  fn token (& self) -> & str
  async fn get_current_user (& self) -> Result < UserProfile >
  async fn search_anime (& self , query : & str) -> Result < Vec < MalAnimeNode > >
  async fn search_anime_limited (& self , query : & str , limit : u32) -> Result < Vec < MalAnimeNode > >
  async fn get_anime (& self , anime_id : u64) -> Result < MalAnimeNode >
  async fn update_episode_progress (& self , anime_id : u64 , episode : u32 , total_episodes : u32 ,) -> Result < UserListStatus >
  ```

### `src/mal/gemini.rs` (Role: general, Lines: 435)
- **Responsibility**: Core general logic in src/mal/gemini.rs
- **Imports**: use anyhow :: { Context , Result } , use reqwest :: Client , use serde :: { Deserialize , Serialize } , use std :: time :: Duration , use super :: parser :: AnimeInfo 
- **Types & Enums**:
  ```rust
  pub struct GeminiClassification
  pub struct GeminiParser
  ```
- **Public Functions & Signatures**:
  ```rust
  fn new (api_key : String , model_override : Option < String >) -> Result < Self >
  fn from_config (config : & super :: auth :: MalConfig) -> Result < Self >
  async fn classify (& self , raw_input : & str) -> Result < Option < GeminiClassification > >
  async fn parse (& self , raw_input : & str) -> Result < Option < AnimeInfo > >
  ```

### `src/mal/handler.rs` (Role: general, Lines: 377)
- **Responsibility**: Core general logic in src/mal/handler.rs
- **Imports**: use anyhow :: { Context , Result } , use chrono :: Local , use std :: io :: { self , Read } , use std :: process :: Command , use super :: auth :: MalAuth , use super :: client :: MalClient , use super :: gemini :: GeminiParser , use super :: history :: { AnimeSyncHistory , AnimeSyncRecord , SyncStatus } , use super :: parser :: AnimeParser , use crate :: logger :: ActivityEvent 
- **Types & Enums**:
  ```rust
  pub struct MalHandler
  ```
- **Public Functions & Signatures**:
  ```rust
  async fn handle_stdin_hook () -> Result < () >
  async fn sync_media (raw_media : & str) -> Result < () >
  async fn sync_media_with_options (raw_media : & str , duration_secs : u64 , watched_secs : u64 , force : bool ,) -> Result < Option < AnimeSyncRecord > >
  ```

### `src/mal/history/tests.rs` (Role: general, Lines: 300)
- **Responsibility**: Core general logic in src/mal/history/tests.rs
- **Imports**: use super :: * 

### `src/mal/history.rs` (Role: general, Lines: 483)
- **Responsibility**: Core general logic in src/mal/history.rs
- **Imports**: use anyhow :: { Context , Result } , use chrono :: Local , use serde :: { Deserialize , Serialize } , use std :: fs :: { create_dir_all , OpenOptions } , use std :: io :: Write , use std :: path :: PathBuf , use crate :: monitor :: PlaybackInterval 
- **Types & Enums**:
  ```rust
  pub enum SyncStatus
  pub struct AnimeSyncRecord
  pub struct ProgressUpdateParams
  pub struct AnimeSyncHistory
  ```
- **Public Functions & Signatures**:
  ```rust
  fn badge (& self) -> (& 'static str , ratatui :: style :: Color)
  fn storage_path () -> PathBuf
  fn deduplicate (records : Vec < AnimeSyncRecord >) -> Vec < AnimeSyncRecord >
  fn load_all () -> Result < Vec < AnimeSyncRecord > >
  fn save_all (records : & [AnimeSyncRecord]) -> Result < () >
  fn upsert (record : AnimeSyncRecord) -> Result < () >
  fn find_record (raw_title : & str , canonical_title : & str , episode : u32 ,) -> Option < AnimeSyncRecord >
  fn update_progress_full (params : ProgressUpdateParams < '_ >) -> Result < () >
  fn update_progress (raw_title : & str , canonical_title : & str , episode : u32 , path : Option < String > , duration_secs : u64 , watched_secs : u64 ,) -> Result < () >
  ```

### `src/mal/overrides.rs` (Role: general, Lines: 287)
- **Responsibility**: Core general logic in src/mal/overrides.rs
- **Imports**: use anyhow :: { Context , Result } , use serde :: { Deserialize , Serialize } , use std :: fs :: OpenOptions , use std :: io :: Write , use std :: path :: { Path , PathBuf } , use super :: history :: { AnimeSyncHistory , AnimeSyncRecord } 
- **Types & Enums**:
  ```rust
  pub struct AnimeOverride
  pub struct AnimeOverrides
  ```
- **Public Functions & Signatures**:
  ```rust
  fn id_for (& self , file_name : & str) -> u64
  fn storage_path () -> PathBuf
  fn load () -> Result < Self >
  fn entries (& self) -> & [AnimeOverride]
  fn save (& self) -> Result < () >
  fn set (& mut self , folder : & Path , mal_anime_id : u64 , canonical_title : & str , total_episodes : Option < u32 > ,) -> AnimeOverride
  fn remove (& mut self , folder : & Path) -> Option < AnimeOverride >
  fn lookup (& self , media_path : & str) -> Option < & AnimeOverride >
  fn reassign_history (folder : & Path , mal_anime_id : u64 , canonical_title : & str , total_episodes : Option < u32 > ,) -> Result < usize >
  ```

### `src/mal/parser.rs` (Role: general, Lines: 175)
- **Responsibility**: Core general logic in src/mal/parser.rs
- **Imports**: use regex :: Regex , use std :: path :: Path 
- **Types & Enums**:
  ```rust
  pub struct AnimeInfo
  pub struct AnimeParser
  ```
- **Public Functions & Signatures**:
  ```rust
  fn parse (raw_name : & str) -> Option < AnimeInfo >
  ```

### `src/mal/resolver.rs` (Role: general, Lines: 129)
- **Responsibility**: Core general logic in src/mal/resolver.rs
- **Imports**: use anyhow :: Result , use super :: candidate :: { build_search_queries , season_from_path , select_best_candidate } , use super :: client :: { MalAnimeNode , MalClient } , use super :: overrides :: AnimeOverrides , use super :: parser :: AnimeInfo 
- **Types & Enums**:
  ```rust
  pub struct ResolvedAnime
  ```
- **Public Functions & Signatures**:
  ```rust
  async fn resolve_anime (client : & MalClient , media_path : & str , info : & AnimeInfo ,) -> Result < Option < ResolvedAnime > >
  async fn resolve_from_override (client : & MalClient , media_path : & str ,) -> Result < Option < ResolvedAnime > >
  ```

### `src/mal.rs` (Role: general, Lines: 21)
- **Responsibility**: Core general logic in src/mal.rs
- **Imports**: pub use auth :: MalAuth , pub use candidate :: select_best_candidate , pub use classifier :: { AnimeClassifier , MediaClassification } , pub use client :: MalClient , pub use gemini :: GeminiParser , pub use handler :: MalHandler , pub use history :: { AnimeSyncHistory , AnimeSyncRecord , SyncStatus } , pub use overrides :: { AnimeOverride , AnimeOverrides } , pub use parser :: { AnimeInfo , AnimeParser } , pub use resolver :: { resolve_anime , ResolvedAnime } 

### `src/monitor/metrics.rs` (Role: general, Lines: 189)
- **Responsibility**: Core general logic in src/monitor/metrics.rs
- **Imports**: use anyhow :: Result , use chrono :: Local , use std :: fs , use std :: path :: Path , use std :: sync :: atomic :: { AtomicBool , Ordering } , use std :: sync :: Arc , use sysinfo :: { CpuRefreshKind , ProcessRefreshKind , System } , use tokio :: time :: { sleep , Duration } , use super :: process :: collect_app_details , pub use super :: process :: AppDetail , use crate :: logger :: { ActivityEvent , LogWriter } 
- **Types & Enums**:
  ```rust
  pub struct MetricsSnapshot
  pub struct MetricsMonitor
  ```
- **Public Functions & Signatures**:
  ```rust
  fn new (writer : LogWriter , interval_secs : u64) -> Self
  fn sample_metrics (sys : & mut System) -> MetricsSnapshot
  async fn run (& mut self , running : Arc < AtomicBool >) -> Result < () >
  ```

### `src/monitor/mod.rs` (Role: general, Lines: 13)
- **Responsibility**: Core general logic in src/monitor/mod.rs
- **Imports**: pub use metrics :: MetricsMonitor , pub use mpv :: MpvMonitor , pub use power :: PowerMonitor , pub use timeline_tracker :: PlaybackInterval , pub use window :: WindowMonitor 

### `src/monitor/mpv.rs` (Role: general, Lines: 252)
- **Responsibility**: Core general logic in src/monitor/mpv.rs
- **Imports**: use anyhow :: Result , use chrono :: Local , use serde :: Deserialize , use std :: env , use std :: path :: { Path , PathBuf } , use std :: sync :: atomic :: { AtomicBool , Ordering } , use std :: sync :: Arc , use tokio :: io :: { AsyncBufReadExt , AsyncWriteExt , BufReader } , use tokio :: net :: UnixStream , use tokio :: time :: { sleep , Duration } , use crate :: logger :: { ActivityEvent , LogWriter } , use super :: mpv_session :: MpvPlaybackSession 
- **Types & Enums**:
  ```rust
  pub struct MpvMonitor
  ```
- **Public Functions & Signatures**:
  ```rust
  fn new (writer : LogWriter) -> Self
  async fn run (& mut self , running : Arc < AtomicBool >) -> Result < () >
  ```

### `src/monitor/mpv_session.rs` (Role: general, Lines: 266)
- **Responsibility**: Core general logic in src/monitor/mpv_session.rs
- **Imports**: use chrono :: Local , use std :: path :: Path , use crate :: mal :: history :: ProgressUpdateParams , use crate :: mal :: { AnimeParser , AnimeSyncHistory , SyncStatus } , use crate :: monitor :: mpv :: MpvMonitor , use crate :: monitor :: timeline_tracker :: { PlaybackInterval , UniqueTimelineTracker } 
- **Types & Enums**:
  ```rust
  pub struct MpvProgressParams
  pub struct MpvPlaybackSession
  ```
- **Public Functions & Signatures**:
  ```rust
  fn new () -> Self
  fn transition_media (& mut self , monitor : & MpvMonitor , new_title : String , new_path : Option < String > ,)
  fn update_title (& mut self , monitor : & MpvMonitor , new_title : String)
  fn update_path (& mut self , monitor : & MpvMonitor , new_path_str : String)
  fn update_pause (& mut self , monitor : & MpvMonitor , paused : bool)
  fn update_time_pos (& mut self , monitor : & MpvMonitor , pos_num : f64)
  fn update_duration (& mut self , dur_num : f64)
  fn finish (& mut self , monitor : & MpvMonitor)
  fn sync_history (& self)
  fn sync_history_progress (params : MpvProgressParams < '_ >)
  ```

### `src/monitor/power.rs` (Role: general, Lines: 202)
- **Responsibility**: Core general logic in src/monitor/power.rs
- **Imports**: use anyhow :: Result , use chrono :: Local , use std :: fs , use std :: path :: Path , use std :: sync :: atomic :: { AtomicBool , Ordering } , use std :: sync :: Arc , use tokio :: time :: { sleep , Duration } , use crate :: logger :: { ActivityEvent , LogWriter } 
- **Types & Enums**:
  ```rust
  pub struct PowerState
  pub struct PowerMonitor
  ```
- **Public Functions & Signatures**:
  ```rust
  fn new (writer : LogWriter) -> Self
  fn read_current_state () -> Option < PowerState >
  async fn run (& mut self , running : Arc < AtomicBool >) -> Result < () >
  ```

### `src/monitor/process.rs` (Role: general, Lines: 279)
- **Responsibility**: Core general logic in src/monitor/process.rs
- **Imports**: use serde :: { Deserialize , Serialize } , use std :: collections :: { BTreeMap , HashSet } , use std :: fs , use std :: path :: Path , use sysinfo :: { Process , System } 
- **Types & Enums**:
  ```rust
  pub struct AppDetail
  ```
- **Public Functions & Signatures**:
  ```rust
  fn is_interpreter (name : & str) -> bool
  fn extract_script_name (cmd : & [String]) -> Option < String >
  fn resolve_process_identity (p : & Process) -> (String , String , String)
  fn get_proc_status_info (pid : u32) -> Option < (u32 , u32 , u64) >
  fn collect_app_details (sys : & System , total_mem : u64) -> (Vec < AppDetail > , Vec < String >)
  ```

### `src/monitor/timeline_tracker.rs` (Role: general, Lines: 297)
- **Responsibility**: Core general logic in src/monitor/timeline_tracker.rs
- **Imports**: use serde :: { Deserialize , Serialize } 
- **Types & Enums**:
  ```rust
  pub struct PlaybackInterval
  pub struct UniqueTimelineTracker
  ```
- **Public Functions & Signatures**:
  ```rust
  fn new () -> Self
  fn intervals (& self) -> & [PlaybackInterval]
  fn merge_interval_lists (a : & [PlaybackInterval] , b : & [PlaybackInterval] ,) -> Vec < PlaybackInterval >
  fn load_intervals (& mut self , intervals : & [PlaybackInterval])
  fn set_threshold_triggered (& mut self , triggered : bool)
  fn set_media (& mut self , media_key : & str , duration : Option < u64 >)
  fn set_duration (& mut self , duration : u64)
  fn record_position (& mut self , current_pos : u64 , is_paused : bool)
  fn add_interval (& mut self , start : u64 , end : u64)
  fn total_unique_secs (& self) -> u64
  fn coverage_pct (& self) -> f32
  fn check_threshold (& mut self , threshold_pct : f32) -> Option < (u64 , u64 , f32) >
  ```

### `src/monitor/window.rs` (Role: general, Lines: 242)
- **Responsibility**: Core general logic in src/monitor/window.rs
- **Imports**: use anyhow :: Result , use chrono :: Local , use serde :: Deserialize , use std :: env , use std :: os :: unix :: net :: UnixStream as StdUnixStream , use std :: path :: PathBuf , use std :: process :: Command , use std :: sync :: atomic :: { AtomicBool , Ordering } , use std :: sync :: Arc , use tokio :: io :: { AsyncBufReadExt , BufReader } , use tokio :: net :: UnixStream , use tokio :: time :: { sleep , Duration } , use crate :: logger :: { ActivityEvent , LogWriter } 
- **Types & Enums**:
  ```rust
  pub struct WindowMonitor
  ```
- **Public Functions & Signatures**:
  ```rust
  fn new (writer : LogWriter) -> Self
  fn get_current_window () -> Option < (String , String) >
  async fn run (& mut self , running : Arc < AtomicBool >) -> Result < () >
  fn find_hyprland_socket_and_sig () -> Option < (String , PathBuf) >
  ```

### `src/plugin.rs` (Role: general, Lines: 114)
- **Responsibility**: Core general logic in src/plugin.rs
- **Imports**: use anyhow :: { Context , Result } , use std :: fs , use std :: os :: unix :: fs :: PermissionsExt , use std :: path :: { Path , PathBuf } , use std :: process :: Stdio , use tokio :: io :: AsyncWriteExt , use tokio :: process :: Command , use tokio :: time :: { timeout , Duration } , use crate :: logger :: ActivityEvent 
- **Types & Enums**:
  ```rust
  pub struct PluginDispatcher
  ```
- **Public Functions & Signatures**:
  ```rust
  fn get_plugins_dir () -> PathBuf
  fn ensure_plugins_dir () -> Result < PathBuf >
  fn list_plugins () -> Vec < PathBuf >
  fn dispatch_event (event : & ActivityEvent)
  ```

### `src/service.rs` (Role: general, Lines: 54)
- **Responsibility**: Core general logic in src/service.rs
- **Imports**: use anyhow :: { Context , Result } , use std :: fs :: { create_dir_all , write } , use std :: process :: Command 
- **Public Functions & Signatures**:
  ```rust
  fn install_user_service () -> Result < () >
  ```

### `src/tui/media_inspector.rs` (Role: tui, Lines: 383)
- **Responsibility**: Core tui logic in src/tui/media_inspector.rs
- **Imports**: use ratatui :: { layout :: { Alignment , Constraint , Direction , Layout , Rect } , style :: { Color , Modifier , Style } , text :: { Line , Span } , widgets :: { Block , Borders , Clear , Gauge , Paragraph , Wrap } , Frame , } , use super :: media_tab :: MediaTabState , use crate :: mal :: SyncStatus 
- **Public Functions & Signatures**:
  ```rust
  fn render_anime_inspector (f : & mut Frame , area : Rect , state : & MediaTabState)
  fn render_force_sync_modal (f : & mut Frame , area : Rect , state : & MediaTabState)
  ```

### `src/tui/media_picker.rs` (Role: tui, Lines: 452)
- **Responsibility**: Core tui logic in src/tui/media_picker.rs
- **Imports**: use ratatui :: { layout :: { Alignment , Constraint , Direction , Layout , Rect } , style :: { Color , Modifier , Style } , text :: { Line , Span } , widgets :: { Block , Borders , Clear , List , ListItem , Paragraph } , Frame , } , use std :: path :: PathBuf , use std :: sync :: mpsc :: { self , Receiver , Sender , TryRecvError } , use std :: time :: { Duration , Instant } , use crate :: mal :: client :: MalAnimeNode , use crate :: mal :: overrides :: reassign_history , use crate :: mal :: { AnimeOverrides , MalAuth , MalClient } 
- **Types & Enums**:
  ```rust
  pub struct MediaPickerState
  ```
- **Public Functions & Signatures**:
  ```rust
  fn new () -> Self
  fn open (& mut self , folder : Option < PathBuf > , seed_query : & str)
  fn close (& mut self)
  fn push_char (& mut self , c : char)
  fn pop_char (& mut self)
  fn retry (& mut self)
  fn needs_query (& self) -> bool
  fn select_next (& mut self)
  fn select_prev (& mut self)
  fn tick (& mut self)
  fn poll (& mut self)
  fn apply (& mut self) -> Option < String >
  fn render_media_picker (f : & mut Frame , area : Rect , state : & MediaPickerState)
  ```

### `src/tui/media_tab/tests.rs` (Role: tui, Lines: 230)
- **Responsibility**: Core tui logic in src/tui/media_tab/tests.rs
- **Imports**: use super :: * 

### `src/tui/media_tab.rs` (Role: tui, Lines: 402)
- **Responsibility**: Core tui logic in src/tui/media_tab.rs
- **Imports**: use chrono :: { Datelike , Local , NaiveDate , NaiveDateTime } , use ratatui :: { layout :: { Constraint , Direction , Layout , Rect } , style :: { Color , Modifier , Style } , text :: { Line , Span } , widgets :: { Block , Borders , List , ListItem , ListState , Paragraph } , Frame , } , use std :: collections :: HashMap , use std :: time :: Instant , use super :: media_inspector :: { render_anime_inspector , render_force_sync_modal } , use crate :: mal :: { AnimeSyncHistory , AnimeSyncRecord , MalHandler , SyncStatus } 
- **Types & Enums**:
  ```rust
  pub enum GroupMode
  pub struct MediaTabState
  ```
- **Public Functions & Signatures**:
  ```rust
  fn next (self) -> Self
  fn label (self) -> & 'static str
  fn new () -> Self
  fn refresh (& mut self)
  fn cycle_group_mode (& mut self)
  fn select_next (& mut self)
  fn select_prev (& mut self)
  fn selected_record (& self) -> Option < & AnimeSyncRecord >
  fn set_status (& mut self , msg : String)
  fn trigger_sync (& mut self , force : bool)
  fn render_media_tab (f : & mut Frame , area : Rect , state : & mut MediaTabState)
  ```

### `src/tui.rs` (Role: tui, Lines: 1598)
- **Responsibility**: Core tui logic in src/tui.rs
- **Imports**: use anyhow :: Result , use chrono :: { DateTime , Local } , use crossterm :: { event :: { self , Event , KeyCode , KeyModifiers } , execute , terminal :: { disable_raw_mode , enable_raw_mode , EnterAlternateScreen , LeaveAlternateScreen } , } , use ratatui :: { backend :: CrosstermBackend , layout :: { Alignment , Constraint , Direction , Layout , Rect } , style :: { Color , Modifier , Style } , text :: { Line , Span } , widgets :: { Block , Borders , Clear , Gauge , List , ListItem , ListState , Paragraph , Wrap } , Terminal , } , use std :: collections :: BTreeMap , use std :: io :: stdout , use std :: process :: { Command , Stdio } , use std :: time :: { Duration , Instant } , use sysinfo :: System , use crate :: exporter :: generate_ai_report , use crate :: logger :: { ActivityEvent , LogWriter } , use crate :: monitor :: metrics :: AppDetail , use crate :: monitor :: { MetricsMonitor , PowerMonitor , WindowMonitor } , use media_picker :: { render_media_picker , MediaPickerState } , use media_tab :: { render_media_tab , MediaTabState } 
- **Public Functions & Signatures**:
  ```rust
  fn run_status_tui () -> Result < () >
  ```

## 4. Execution Lifecycle Trace
1. **Startup**: Entrypoint parses CLI flags & dispatches command.
2. **Execution**: Core domain logic processes inputs and evaluates rules.
3. **Persistence / I/O**: Domain logic calls infra for disk/terminal I/O.
4. **Exit**: Graceful termination with standard exit codes.

## 5. Verification Commands
```bash
cargo build --release --target x86_64-unknown-linux-gnu
cargo test --all-targets
cargo clippy --all-targets -- -D warnings && cargo fmt --check
```
