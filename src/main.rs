mod analytics;
mod css;
mod db;
mod editor;
mod error;
mod front_matter;
mod handler;
mod rss;
mod state;
mod template;
mod tui;

use anyhow::Context;
use axum::{Router, http::StatusCode, middleware, response::Redirect, routing::get};
use clap::Parser;
use sqlx::SqlitePool;
use state::AppState;
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::RwLock;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Parser, Debug)]
#[command(name = "md-server", about = "Serve markdown files as HTML")]
struct Args {
    /// Port to listen on
    #[arg(short, long, env = "PORT", default_value = "3000")]
    port: u16,

    /// Host address to bind to
    #[arg(long, env = "HOST", default_value = "0.0.0.0")]
    host: String,

    /// Path to the www root directory.
    /// Defaults to a `www` directory adjacent to the server binary.
    #[arg(long, env = "WWW_ROOT")]
    root: Option<PathBuf>,

    /// Base URL prepended to item links in generated RSS feeds (e.g. "https://example.com").
    /// If unset, RSS item links will be relative paths.
    #[arg(long, env = "BASE_URL")]
    base_url: Option<String>,

    /// Run in headless mode (no TUI). Useful for Docker / systemd deployments.
    #[arg(long, default_value = "false")]
    headless: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "md_server=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Locate the directory that contains the binary — .env and the DB live here.
    let exe = std::env::current_exe().context("Cannot determine binary path")?;
    let exe_dir = exe
        .parent()
        .context("Binary has no parent directory")?
        .to_path_buf();

    // Load .env from the binary's directory (silently ignored if absent).
    let env_path = exe_dir.join(".env");
    dotenvy::from_path(&env_path).ok();

    let args = Args::parse();

    let www_root = match args.root {
        Some(path) => path,
        None => exe_dir.join("www"),
    };

    tracing::info!("www root: {}", www_root.display());
    if !www_root.exists() {
        tracing::warn!("www root does not exist yet: {}", www_root.display());
    }

    // Initialise the SQLite database (creates file + schema if needed).
    let db_path = exe_dir.join("md-server.db");
    tracing::info!("Database: {}", db_path.display());
    let db = db::init_pool(&db_path)
        .await
        .context("Failed to initialise database")?;

    if args.headless {
        tracing::info!("Headless mode — TUI disabled");
        let state = build_state(www_root, args.base_url, db).await?;
        run_http_server(args.host, args.port, state).await?;
    } else {
        tui::run(tui::TuiConfig {
            host: args.host,
            port: args.port,
            db,
            env_path,
            www_root,
            base_url: args.base_url,
        })
        .await?;
    }

    Ok(())
}

// ── Server helpers (pub(crate) so tui.rs can call them) ──────────────────────

pub(crate) async fn build_state(
    www_root: PathBuf,
    base_url: Option<String>,
    db: SqlitePool,
) -> anyhow::Result<AppState> {
    let canonical_root = tokio::fs::canonicalize(&www_root)
        .await
        .unwrap_or_else(|_| www_root.clone());

    Ok(AppState {
        www_root,
        canonical_root,
        base_url,
        db,
        sessions: Arc::new(RwLock::new(HashMap::new())),
    })
}

pub(crate) fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(|| async { StatusCode::OK }))
        // Redirect /edit/ → /edit to avoid the matchit empty-catchall gap.
        .route("/edit/", get(|| async { Redirect::permanent("/edit") }))
        .merge(editor::router(state.clone()))
        .fallback(handler::handle)
        // Analytics middleware — skips /healthz and /edit/* internally.
        .layer(middleware::from_fn_with_state(
            state.clone(),
            analytics::log_request,
        ))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(CatchPanicLayer::new())
}

pub(crate) async fn run_http_server(
    host: String,
    port: u16,
    state: AppState,
) -> anyhow::Result<()> {
    // Periodically evict expired sessions so the map doesn't grow unboundedly
    // when browsers close without logging out.
    let sessions = state.sessions.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30 * 60));
        loop {
            interval.tick().await;
            sessions
                .write()
                .await
                .retain(|_, created| created.elapsed() < editor::SESSION_TTL);
        }
    });

    let addr = format!("{host}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Cannot bind to {addr}"))?;

    tracing::info!("Listening on http://{addr}");

    let app = build_router(state);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("Server error")
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigterm =
            signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                if let Err(e) = result { tracing::error!("ctrl-c error: {}", e); }
            }
            _ = sigterm.recv() => {
                tracing::info!("Received SIGTERM");
            }
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.ok();
    }
    tracing::info!("Shutting down gracefully");
}
