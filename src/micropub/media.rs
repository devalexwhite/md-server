use axum::{
    Json,
    extract::{Multipart, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::Local;

use crate::{db, db::TokenRecord, state::AppState};
use super::handlers::check_scope;
use super::types::MicropubError;

/// POST /micropub/media — accepts a multipart/form-data upload with a `file` field.
/// Saves the file into `{www_root}/{media_dir}/{YYYY}/{MM}/{filename}` and
/// returns 201 with a `Location` header pointing to the uploaded file's URL.
pub async fn post_media(
    State(state): State<AppState>,
    axum::Extension(token): axum::Extension<TokenRecord>,
    mut multipart: Multipart,
) -> Response {
    if let Some(r) = check_scope(&token, "media") { return r; }

    let media_dir = {
        let v = db::get_micropub_setting(&state.db, "media_dir").await.unwrap_or_default();
        if v.is_empty() { "_media".to_string() } else { v }
    };

    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() != Some("file") {
            continue;
        }

        let original_name = field
            .file_name()
            .unwrap_or("upload")
            .to_string();

        let content_type = field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string();

        if !is_allowed_media_type(&content_type) {
            return (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                Json(MicropubError::new(
                    "invalid_request",
                    "File type not permitted. Allowed: image/*, video/*, audio/*, application/pdf",
                )),
            )
                .into_response();
        }

        let data = match field.bytes().await {
            Ok(b) => b,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(MicropubError::new("invalid_request", "Failed to read uploaded file")),
                )
                    .into_response()
            }
        };

        // Build storage path: {canonical_root}/{media_dir}/{YYYY}/{MM}/
        let now = Local::now();
        let storage_dir = state.canonical_root
            .join(media_dir.trim_matches('/'))
            .join(now.format("%Y/%m").to_string());

        if let Err(e) = tokio::fs::create_dir_all(&storage_dir).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MicropubError::new("server_error", &e.to_string())),
            )
                .into_response();
        }

        // Verify the created directory is still within canonical_root
        let canonical_storage = match tokio::fs::canonicalize(&storage_dir).await {
            Ok(p) if p.starts_with(&state.canonical_root) => p,
            _ => {
                return (
                    StatusCode::FORBIDDEN,
                    Json(MicropubError::new("forbidden", "Storage path escapes www root")),
                )
                    .into_response()
            }
        };

        let safe_name = sanitize_media_filename(&original_name);
        let final_name = find_available_filename(&canonical_storage, &safe_name).await;
        let dest = canonical_storage.join(&final_name);

        if let Err(e) = tokio::fs::write(&dest, data).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MicropubError::new("server_error", &e.to_string())),
            )
                .into_response();
        }

        let url_path = format!("/{}/{}/{}", media_dir.trim_matches('/'), now.format("%Y/%m"), final_name);
        let location = match &state.base_url {
            Some(base) => format!("{}{}", base.trim_end_matches('/'), url_path),
            None => url_path,
        };

        tracing::info!("Micropub media: saved {}", dest.display());
        return (StatusCode::CREATED, [(header::LOCATION, location)]).into_response();
    }

    (
        StatusCode::BAD_REQUEST,
        Json(MicropubError::new(
            "invalid_request",
            "No `file` field found in multipart body",
        )),
    )
        .into_response()
}

/// Sanitize a client-supplied filename to ASCII alphanumeric + `-._` only.
/// Strips path separators and other dangerous characters.
fn sanitize_media_filename(name: &str) -> String {
    // Take only the last path component (strip any directory prefix)
    let basename = name
        .rsplit(|c| c == '/' || c == '\\')
        .next()
        .unwrap_or(name);

    let sanitized: String = basename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();

    // Collapse consecutive hyphens, strip leading/trailing hyphens/dots
    let parts: Vec<&str> = sanitized
        .split('-')
        .filter(|s| !s.is_empty())
        .collect();
    let joined = parts.join("-");

    // Ensure the name isn't empty or just dots
    if joined.trim_matches('.').is_empty() {
        "upload".to_string()
    } else {
        joined
    }
}

/// Find a filename that does not yet exist in `dir`.
/// If `name` already exists, appends `-1`, `-2`, … up to 100 attempts.
async fn find_available_filename(dir: &std::path::Path, name: &str) -> String {
    let path = dir.join(name);
    if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
        return name.to_string();
    }

    // Split name into stem and extension
    let (stem, ext) = match name.rfind('.') {
        Some(dot) => (&name[..dot], &name[dot..]),
        None => (name, ""),
    };

    for i in 1..=100 {
        let candidate = format!("{}-{}{}", stem, i, ext);
        let candidate_path = dir.join(&candidate);
        if !tokio::fs::try_exists(&candidate_path).await.unwrap_or(false) {
            return candidate;
        }
    }

    // Fallback: timestamp-based name
    format!(
        "{}-{}{}", stem,
        chrono::Local::now().format("%Y%m%d%H%M%S"),
        ext
    )
}

/// Check that the MIME type is in the allowed set for media uploads.
fn is_allowed_media_type(content_type: &str) -> bool {
    let base = content_type.split(';').next().unwrap_or("").trim();
    base.starts_with("image/")
        || base.starts_with("video/")
        || base.starts_with("audio/")
        || base == "application/pdf"
}
