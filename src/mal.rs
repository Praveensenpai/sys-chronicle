pub mod auth;
pub mod client;
pub mod gemini;
pub mod handler;
pub mod history;
pub mod parser;

pub use auth::MalAuth;
pub use client::MalClient;
pub use gemini::GeminiParser;
pub use handler::MalHandler;
pub use history::{AnimeSyncHistory, AnimeSyncRecord, SyncStatus};
pub use parser::{AnimeInfo, AnimeParser};
