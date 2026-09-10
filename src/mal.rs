pub mod auth;
pub mod candidate;
pub mod classifier;
pub mod client;
pub mod gemini;
pub mod handler;
pub mod history;
pub mod parser;

pub use auth::MalAuth;
pub use candidate::select_best_candidate;
pub use classifier::{AnimeClassifier, MediaClassification};
pub use client::MalClient;
pub use gemini::GeminiParser;
pub use handler::MalHandler;
pub use history::{AnimeSyncHistory, AnimeSyncRecord, SyncStatus};
pub use parser::{AnimeInfo, AnimeParser};
