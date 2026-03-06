pub(crate) mod handlers;
pub(crate) mod media;
pub(crate) mod types;

use axum::{
    Router,
    extract::{DefaultBodyLimit, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
};
use sha2::{Digest, Sha256};

use crate::{db, state::AppState};
use types::MicropubError;

pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/micropub", get(handlers::get_query).post(handlers::post_endpoint))
        .route("/micropub/media", post(media::post_media))
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024)) // 50 MB for media uploads
        .route_layer(middleware::from_fn_with_state(state, require_bearer))
}

// ── Bearer auth middleware ────────────────────────────────────────────────────

async fn require_bearer(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let raw_token = match extract_bearer_token(req.headers()) {
        Some(t) => t,
        None => return bearer_challenge("Bearer token required"),
    };

    let hash = sha256_hex(&raw_token);
    match db::verify_micropub_token(&state.db, &hash).await {
        Ok(Some(record)) => {
            req.extensions_mut().insert(record);
            next.run(req).await
        }
        Ok(None) => bearer_challenge("Invalid or expired token"),
        Err(e) => {
            tracing::error!("Micropub token verification failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MicropubError::new("server_error", "Internal error")),
            )
                .into_response()
        }
    }
}

fn extract_bearer_token(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(|t| t.trim().to_string())
}

fn bearer_challenge(msg: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Bearer")],
        Json(MicropubError::new("unauthorized", msg)),
    )
        .into_response()
}

// ── Shared helpers ────────────────────────────────────────────────────────────

/// SHA-256 hex digest — used for token storage and lookup.
pub fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

/// Generate a new random 256-bit hex token (same pattern as editor sessions).
pub fn new_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
