use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::Instant,
};
use sqlx::SqlitePool;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppState {
    pub www_root: PathBuf,
    /// Canonicalized (symlink-resolved) version of `www_root`.
    /// Used for security checks in path validation.
    pub canonical_root: PathBuf,
    /// Optional base URL (e.g. "https://example.com") prepended to item links in RSS feeds.
    pub base_url: Option<String>,
    /// SQLite connection pool — shared across all request handlers.
    pub db: SqlitePool,
    /// Active editor sessions: token → last-used instant.
    pub sessions: Arc<RwLock<HashMap<String, Instant>>>,
}
