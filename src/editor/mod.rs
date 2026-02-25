mod handlers;
mod template;

use axum::{
    Form, Router,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{delete, get, post},
};
use serde::Deserialize;
use std::time::{Duration, Instant};

use crate::state::AppState;

/// Session cookie name.
const SESSION_COOKIE: &str = "ed_session";
/// Session lifetime (24 hours, sliding).
const SESSION_TTL: Duration = Duration::from_secs(24 * 3600);

// ── Router ────────────────────────────────────────────────────────────────────

/// Build the editor router with full `/edit/*` paths.
/// Uses `merge` (not `nest`) in `main.rs` to avoid matchit's empty-catchall
/// gap which causes `/edit/` to fall through to the fallback handler.
pub fn router(state: AppState) -> Router<AppState> {
    let public = Router::new()
        .route("/edit/login", get(get_login).post(post_login));

    let protected = Router::new()
        .route("/edit", get(handlers::get_dashboard))
        .route("/edit/open", get(handlers::get_editor))
        .route("/edit/save", post(handlers::post_save))
        .route("/edit/preview", post(handlers::post_preview))
        .route("/edit/new-file", post(handlers::post_new_file))
        .route("/edit/new-dir", post(handlers::post_new_dir))
        .route("/edit/delete", delete(handlers::delete_file))
        .route("/edit/rename", post(handlers::post_rename))
        .route("/edit/logout", post(post_logout))
        .route_layer(middleware::from_fn_with_state(state, require_auth));

    Router::new().merge(public).merge(protected)
}

// ── Auth middleware ───────────────────────────────────────────────────────────

async fn require_auth(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let token = extract_session_cookie(req.headers());

    if let Some(tok) = token {
        // Use a single write lock for both the validity check and the expiry slide
        // to avoid a TOCTOU race where a concurrent logout removes the session
        // between the read and write.
        let valid = {
            let mut sessions = state.sessions.write().await;
            if let Some(exp) = sessions.get(&tok) {
                if exp.elapsed() < SESSION_TTL {
                    sessions.insert(tok.clone(), Instant::now());
                    true
                } else {
                    sessions.remove(&tok);
                    false
                }
            } else {
                false
            }
        };
        if valid {
            return next.run(req).await;
        }
    }

    Redirect::to("/edit/login").into_response()
}

// ── Login / logout ────────────────────────────────────────────────────────────

async fn get_login() -> Response {
    Html(template::login_page(None).into_string()).into_response()
}

#[derive(Deserialize)]
struct LoginForm {
    username: String,
    password: String,
}

async fn post_login(State(state): State<AppState>, Form(form): Form<LoginForm>) -> Response {
    let ok = crate::db::verify_user(&state.db, &form.username, &form.password).await;

    if ok {
        let token = new_session_token();
        state
            .sessions
            .write()
            .await
            .insert(token.clone(), Instant::now());

        let cookie = format!(
            "{}={}; Path=/edit; HttpOnly; SameSite=Strict; Max-Age={}",
            SESSION_COOKIE,
            token,
            SESSION_TTL.as_secs()
        );
        (
            StatusCode::SEE_OTHER,
            [
                (header::SET_COOKIE, cookie),
                (header::LOCATION, "/edit".to_string()),
            ],
        )
            .into_response()
    } else {
        Html(template::login_page(Some("Invalid username or password.")).into_string())
            .into_response()
    }
}

async fn post_logout(State(state): State<AppState>, req: Request) -> Response {
    if let Some(tok) = extract_session_cookie(req.headers()) {
        state.sessions.write().await.remove(&tok);
    }
    let clear = format!(
        "{}=; Path=/edit; HttpOnly; SameSite=Strict; Max-Age=0",
        SESSION_COOKIE
    );
    (
        StatusCode::SEE_OTHER,
        [
            (header::SET_COOKIE, clear),
            (header::LOCATION, "/edit/login".to_string()),
        ],
    )
        .into_response()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn extract_session_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    let cookie_str = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie_str.split(';') {
        let part = part.trim();
        if let Some(val) = part.strip_prefix(&format!("{}=", SESSION_COOKIE)) {
            return Some(val.to_string());
        }
    }
    None
}

fn new_session_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
