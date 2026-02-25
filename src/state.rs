use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::Instant,
};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppState {
    pub www_root: PathBuf,
    /// Canonicalized (symlink-resolved) version of `www_root`.
    /// Used for security checks in path validation.
    pub canonical_root: PathBuf,
    /// Optional base URL (e.g. "https://example.com") prepended to item links in RSS feeds.
    pub base_url: Option<String>,
    /// Editor config; `None` means the editor is disabled (env vars not set).
    pub editor: Option<EditorConfig>,
}

/// Configuration and live session store for the /edit dashboard.
#[derive(Clone)]
pub struct EditorConfig {
    pub username: String,
    pub password: String,
    /// Active sessions: token → expiry instant.
    pub sessions: Arc<RwLock<HashMap<String, Instant>>>,
}

impl EditorConfig {
    pub fn new(username: String, password: String) -> Self {
        Self {
            username,
            password,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}
