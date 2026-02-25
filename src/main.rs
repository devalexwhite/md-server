mod css;
mod editor;
mod error;
mod front_matter;
mod handler;
mod rss;
mod state;
mod template;

use anyhow::Context;
use axum::{http::StatusCode, response::Redirect, routing::get, Router};
use clap::Parser;
use state::{AppState, EditorConfig};
use std::path::PathBuf;
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

    /// Editor dashboard username. If unset, the /edit dashboard is disabled.
    #[arg(long, env = "EDITOR_USERNAME")]
    editor_username: Option<String>,

    /// Editor dashboard password. If unset, the /edit dashboard is disabled.
    #[arg(long, env = "EDITOR_PASSWORD")]
    editor_password: Option<String>,
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

    // Load .env file if present (silently ignored if absent).
    dotenvy::dotenv().ok();

    let args = Args::parse();

    let www_root = match args.root {
        Some(path) => path,
        None => {
            let exe = std::env::current_exe().context("Cannot determine binary path")?;
            exe.parent()
                .context("Binary has no parent directory")?
                .join("www")
        }
    };

    tracing::info!("www root: {}", www_root.display());
    if !www_root.exists() {
        tracing::warn!("www root does not exist yet: {}", www_root.display());
    }

    // Resolve symlinks in www_root for security comparisons at request time.
    // Falls back to the lexical path if the directory doesn't exist yet.
    let canonical_root = tokio::fs::canonicalize(&www_root)
        .await
        .unwrap_or_else(|_| www_root.clone());

    let editor = match (args.editor_username, args.editor_password) {
        (Some(u), Some(p)) => {
            tracing::info!("Editor dashboard enabled at /edit");
            Some(EditorConfig::new(u, p))
        }
        _ => {
            tracing::info!("Editor dashboard disabled (EDITOR_USERNAME/EDITOR_PASSWORD not set)");
            None
        }
    };

    let state = AppState {
        www_root,
        canonical_root,
        base_url: args.base_url,
        editor,
    };

    // CatchPanicLayer is outermost so it recovers from panics anywhere in the stack.
    let app = Router::new()
        .route("/healthz", get(|| async { StatusCode::OK }))
        // Redirect /edit/ → /edit to avoid the matchit empty-catchall gap in nest().
        .route("/edit/", get(|| async { Redirect::permanent("/edit") }))
        .merge(editor::router(state.clone()))
        .fallback(handler::handle)
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(CatchPanicLayer::new());

    let addr = format!("{}:{}", args.host, args.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Cannot bind to {addr}"))?;

    tracing::info!("Listening on http://{addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("Server error")?;

    Ok(())
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
