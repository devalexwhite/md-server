use axum::{
    Json,
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::Local;
use std::collections::HashMap;

use crate::{
    db::{self, TokenRecord},
    editor::handlers::{resolve_read_path, resolve_write_path},
    front_matter::{self, FrontMatter, ParsedDoc, write_front_matter},
    state::AppState,
};
use super::types::{
    CreateEntry, MicropubConfig, MicropubError, MicropubRequest, PostTypeInfo, SourceProperties,
    SourceResponse, UpdateRequest,
};

// ── Shared helpers ─────────────────────────────────────────────────────────────

/// Return a 403 response if the token's scope doesn't include `required`.
pub(crate) fn check_scope(token: &TokenRecord, required: &str) -> Option<Response> {
    if token.scope.split_whitespace().any(|s| s == required) {
        None
    } else {
        Some((
            StatusCode::FORBIDDEN,
            Json(MicropubError::new(
                "insufficient_scope",
                &format!("Token lacks {} scope", required),
            )),
        )
            .into_response())
    }
}

// ── GET /micropub ─────────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct QueryParams {
    pub q: Option<String>,
    pub url: Option<String>,
}

pub async fn get_query(
    State(state): State<AppState>,
    axum::Extension(_token): axum::Extension<TokenRecord>,
    Query(params): Query<QueryParams>,
) -> Response {
    match params.q.as_deref() {
        Some("config") => {
            let base = state.base_url.as_deref().unwrap_or("");
            Json(MicropubConfig {
                media_endpoint: format!("{}/micropub/media", base),
                syndicate_to: vec![],
                post_types: vec![
                    PostTypeInfo {
                        post_type: "note".to_string(),
                        name: "Note".to_string(),
                    },
                    PostTypeInfo {
                        post_type: "article".to_string(),
                        name: "Article".to_string(),
                    },
                ],
            })
            .into_response()
        }
        Some("source") => {
            let url = match &params.url {
                Some(u) => u.clone(),
                None => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(MicropubError::new(
                            "invalid_request",
                            "url parameter required for q=source",
                        )),
                    )
                        .into_response()
                }
            };
            handle_source_query(&state, &url).await
        }
        Some("syndicate-to") => {
            Json(serde_json::json!({ "syndicate-to": [] })).into_response()
        }
        Some(q) => (
            StatusCode::BAD_REQUEST,
            Json(MicropubError::new(
                "invalid_request",
                &format!("Unsupported query type: {}", q),
            )),
        )
            .into_response(),
        None => {
            let base = state.base_url.as_deref().unwrap_or("");
            Json(serde_json::json!({
                "media-endpoint": format!("{}/micropub/media", base)
            }))
            .into_response()
        }
    }
}

async fn handle_source_query(state: &AppState, url: &str) -> Response {
    let (_fs_path, ParsedDoc { front_matter, content }) = match load_post_by_url(state, url).await {
        Ok(pair) => pair,
        Err(r) => return r,
    };
    let rel = url_to_rel_path(state, url).unwrap_or_default();

    let canonical_url = match &state.base_url {
        Some(base) => format!(
            "{}/{}",
            base.trim_end_matches('/'),
            rel.trim_end_matches(".md")
        ),
        None => format!("/{}", rel.trim_end_matches(".md")),
    };

    let response = SourceResponse {
        post_type: vec!["h-entry".to_string()],
        properties: SourceProperties {
            name: front_matter
                .title
                .as_deref()
                .map(|t| vec![t.to_string()])
                .unwrap_or_default(),
            content: vec![content.trim().to_string()],
            category: front_matter.tags.unwrap_or_default(),
            published: front_matter
                .date
                .as_deref()
                .map(|d| vec![d.to_string()])
                .unwrap_or_default(),
            url: vec![canonical_url],
        },
    };

    Json(response).into_response()
}

// ── POST /micropub ────────────────────────────────────────────────────────────

pub async fn post_endpoint(
    State(state): State<AppState>,
    axum::Extension(token): axum::Extension<TokenRecord>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let request = if content_type.starts_with("application/json") {
        parse_json_body(&body)
    } else if content_type.starts_with("application/x-www-form-urlencoded") {
        parse_form_body(&body)
    } else {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Json(MicropubError::new(
                "invalid_request",
                "Content-Type must be application/json or application/x-www-form-urlencoded",
            )),
        )
            .into_response();
    };

    match request {
        Ok(MicropubRequest::Create(entry)) => {
            if let Some(r) = check_scope(&token, "create") { return r; }
            handle_create(&state, entry).await
        }
        Ok(MicropubRequest::Update(update)) => {
            if let Some(r) = check_scope(&token, "update") { return r; }
            handle_update(&state, update).await
        }
        Ok(MicropubRequest::Delete { url }) => {
            if let Some(r) = check_scope(&token, "delete") { return r; }
            handle_delete(&state, &url, true).await
        }
        Ok(MicropubRequest::Undelete { url }) => {
            if let Some(r) = check_scope(&token, "delete") { return r; }
            handle_delete(&state, &url, false).await
        }
        Err(e) => e,
    }
}

// ── Post loader helper ────────────────────────────────────────────────────────

/// Resolve a Micropub post URL → parsed file. Returns the fs path and ParsedDoc,
/// or an error Response if the URL cannot be resolved or the file cannot be read.
async fn load_post_by_url(
    state: &AppState,
    url: &str,
) -> Result<(std::path::PathBuf, ParsedDoc), Response> {
    let rel = match url_to_rel_path(state, url) {
        Some(r) => r,
        None => return Err((
            StatusCode::BAD_REQUEST,
            Json(MicropubError::new("invalid_request", "Cannot resolve URL to a file")),
        ).into_response()),
    };

    let fs_path = match resolve_read_path(state, &rel).await {
        Ok(p) => p,
        Err(_) => return Err((
            StatusCode::NOT_FOUND,
            Json(MicropubError::new("invalid_request", "Post not found")),
        ).into_response()),
    };

    let raw = match tokio::fs::read_to_string(&fs_path).await {
        Ok(r) => r,
        Err(_) => return Err((
            StatusCode::NOT_FOUND,
            Json(MicropubError::new("invalid_request", "Post not found")),
        ).into_response()),
    };

    Ok((fs_path, front_matter::parse(&raw)))
}

// ── Create ────────────────────────────────────────────────────────────────────

async fn handle_create(state: &AppState, entry: CreateEntry) -> Response {
    let post_dir = {
        let v = db::get_micropub_setting(&state.db, "post_dir").await.unwrap_or_default();
        if v.is_empty() { "posts".to_string() } else { v }
    };

    let now = Local::now();
    let date_str = now.format("%Y-%m-%d").to_string();

    // Determine slug: mp-slug override (sanitized) > title-derived > timestamp
    let slug = entry
        .slug
        .as_deref()
        .map(slugify)
        .or_else(|| entry.name.as_deref().map(slugify))
        .unwrap_or_else(|| now.format("%H%M%S").to_string());

    let filename = format!("{}-{}.md", date_str, slug);
    let rel_path = format!("{}/{}", post_dir.trim_matches('/'), filename);

    let fs_path = match resolve_write_path(state, &rel_path).await {
        Ok(p) => p,
        Err(r) => return r,
    };

    if let Some(parent) = fs_path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MicropubError::new("server_error", &e.to_string())),
            )
                .into_response();
        }
    }

    let published_date = entry
        .published
        .as_deref()
        .map(normalize_date)
        .unwrap_or(date_str);

    let fm = FrontMatter {
        title: entry.name.clone(),
        date: Some(published_date),
        draft: Some(false),
        tags: if entry.tags.is_empty() {
            None
        } else {
            Some(entry.tags)
        },
        ..Default::default()
    };

    let file_content = match write_front_matter(&fm, &format!("\n{}\n", entry.content.trim())) {
        Ok(s) => s,
        Err(e) => return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(MicropubError::new("server_error", &format!("Failed to serialize front matter: {}", e))),
        ).into_response(),
    };

    // Use create_new to atomically detect conflicts without a TOCTOU race.
    let write_result = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&fs_path)
        .await;

    match write_result {
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return (
                StatusCode::CONFLICT,
                Json(MicropubError::new(
                    "invalid_request",
                    &format!("A post already exists at: {}", rel_path),
                )),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MicropubError::new("server_error", &e.to_string())),
            )
                .into_response();
        }
        Ok(mut file) => {
            use tokio::io::AsyncWriteExt;
            if let Err(e) = file.write_all(file_content.as_bytes()).await {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(MicropubError::new("server_error", &e.to_string())),
                )
                    .into_response();
            }
        }
    }

    // Build canonical Location URL
    let stem = filename.strip_suffix(".md").unwrap_or(&filename);
    let url_path = format!("/{}/{}", post_dir.trim_matches('/'), stem);
    let location = match &state.base_url {
        Some(base) => format!("{}{}", base.trim_end_matches('/'), url_path),
        None => url_path,
    };

    tracing::info!("Micropub: created {}", rel_path);

    (StatusCode::CREATED, [(header::LOCATION, location)]).into_response()
}

// ── Update ────────────────────────────────────────────────────────────────────

async fn handle_update(state: &AppState, update: UpdateRequest) -> Response {
    let rel = url_to_rel_path(state, &update.url).unwrap_or_default();
    let (fs_path, ParsedDoc { mut front_matter, mut content }) =
        match load_post_by_url(state, &update.url).await {
            Ok(pair) => pair,
            Err(r) => return r,
        };

    // Apply replace operations
    for (prop, values) in &update.replace {
        apply_property(&mut front_matter, &mut content, prop, values, UpdateOp::Replace);
    }

    // Apply add operations
    for (prop, values) in &update.add {
        apply_property(&mut front_matter, &mut content, prop, values, UpdateOp::Add);
    }

    // Apply delete operations (remove properties entirely)
    for prop in &update.delete {
        match prop.as_str() {
            "name" => front_matter.title = None,
            "summary" => front_matter.summary = None,
            "category" => front_matter.tags = None,
            "published" => front_matter.date = None,
            "post-status" => front_matter.draft = None,
            _ => {} // unknown properties silently ignored
        }
    }

    let new_file = match write_front_matter(&front_matter, &content) {
        Ok(s) => s,
        Err(e) => return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(MicropubError::new("server_error", &format!("Failed to serialize front matter: {}", e))),
        ).into_response(),
    };
    if let Err(e) = tokio::fs::write(&fs_path, new_file.as_bytes()).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(MicropubError::new("server_error", &e.to_string())),
        )
            .into_response();
    }

    tracing::info!("Micropub: updated {}", rel);
    StatusCode::OK.into_response()
}

enum UpdateOp {
    Replace,
    Add,
}

fn apply_property(
    fm: &mut FrontMatter,
    content: &mut String,
    prop: &str,
    values: &[serde_json::Value],
    op: UpdateOp,
) {
    match prop {
        "name" => {
            if let Some(v) = values.first().and_then(|v| v.as_str()) {
                fm.title = Some(v.to_string());
            }
        }
        "content" => {
            if let Some(new_content) = values.first().map(extract_content_value) {
                *content = format!("\n{}\n", new_content.trim());
            }
        }
        "category" => {
            let new_tags: Vec<String> = values
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect();
            match op {
                UpdateOp::Replace => {
                    fm.tags = if new_tags.is_empty() { None } else { Some(new_tags) };
                }
                UpdateOp::Add => {
                    let existing = fm.tags.get_or_insert_with(Vec::new);
                    for tag in new_tags {
                        if !existing.contains(&tag) {
                            existing.push(tag);
                        }
                    }
                }
            }
        }
        "published" => {
            if let Some(v) = values.first().and_then(|v| v.as_str()) {
                fm.date = Some(normalize_date(v));
            }
        }
        "post-status" => {
            if let Some(v) = values.first().and_then(|v| v.as_str()) {
                fm.draft = Some(v == "draft");
            }
        }
        "summary" => {
            if let Some(v) = values.first().and_then(|v| v.as_str()) {
                fm.summary = Some(v.to_string());
            }
        }
        _ => {} // unknown properties silently ignored
    }
}

fn extract_content_value(v: &serde_json::Value) -> String {
    // Content can be a plain string or {"html":"..."} or {"markdown":"..."}
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    if let Some(obj) = v.as_object() {
        if let Some(html) = obj.get("html").and_then(|h| h.as_str()) {
            return html.to_string();
        }
        if let Some(md) = obj.get("markdown").and_then(|m| m.as_str()) {
            return md.to_string();
        }
    }
    String::new()
}

// ── Delete / Undelete ─────────────────────────────────────────────────────────

/// Soft-delete: set `draft: true` (or `draft: false` for undelete).
/// This makes the post invisible to public readers (existing serve_markdown
/// already returns 404 for drafts) without permanently deleting the file.
async fn handle_delete(state: &AppState, url: &str, make_draft: bool) -> Response {
    let rel = url_to_rel_path(state, url).unwrap_or_default();
    let (fs_path, ParsedDoc { mut front_matter, content }) =
        match load_post_by_url(state, url).await {
            Ok(pair) => pair,
            Err(r) => return r,
        };
    front_matter.draft = Some(make_draft);

    let new_file = match write_front_matter(&front_matter, &content) {
        Ok(s) => s,
        Err(e) => return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(MicropubError::new("server_error", &format!("Failed to serialize front matter: {}", e))),
        ).into_response(),
    };
    if let Err(e) = tokio::fs::write(&fs_path, new_file.as_bytes()).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(MicropubError::new("server_error", &e.to_string())),
        )
            .into_response();
    }

    let action = if make_draft { "deleted (set draft)" } else { "undeleted" };
    tracing::info!("Micropub: {} {}", action, rel);
    StatusCode::OK.into_response()
}

// ── Body parsers ──────────────────────────────────────────────────────────────

fn parse_form_body(body: &[u8]) -> Result<MicropubRequest, Response> {
    let pairs: Vec<(String, String)> = form_urlencoded::parse(body)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    let get = |key: &str| -> Option<String> {
        pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    };

    // Collect all values for a key, including `key[]` bracket convention.
    let get_all = |key: &str| -> Vec<String> {
        let bracket = format!("{}[]", key);
        pairs
            .iter()
            .filter(|(k, _)| k == key || k == &bracket)
            .map(|(_, v)| v.clone())
            .collect()
    };

    let action = get("action");
    match action.as_deref() {
        None | Some("create") => {
            let h = get("h").unwrap_or_else(|| "entry".to_string());
            if h != "entry" {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(MicropubError::new("invalid_request", "Only h=entry is supported")),
                )
                    .into_response());
            }
            Ok(MicropubRequest::Create(CreateEntry {
                name: get("name"),
                content: get("content").unwrap_or_default(),
                tags: get_all("category"),
                slug: get("mp-slug"),
                published: get("published"),
            }))
        }
        Some("update") => {
            // Form-encoded update is not well-defined in the spec; require JSON.
            Err((
                StatusCode::BAD_REQUEST,
                Json(MicropubError::new(
                    "invalid_request",
                    "Update action requires application/json body",
                )),
            )
                .into_response())
        }
        Some("delete") => {
            let url = get("url").ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(MicropubError::new("invalid_request", "url field required for delete")),
                )
                    .into_response()
            })?;
            Ok(MicropubRequest::Delete { url })
        }
        Some("undelete") => {
            let url = get("url").ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(MicropubError::new(
                        "invalid_request",
                        "url field required for undelete",
                    )),
                )
                    .into_response()
            })?;
            Ok(MicropubRequest::Undelete { url })
        }
        Some(a) => Err((
            StatusCode::BAD_REQUEST,
            Json(MicropubError::new(
                "invalid_request",
                &format!("Unknown action: {}", a),
            )),
        )
            .into_response()),
    }
}

fn parse_json_body(body: &[u8]) -> Result<MicropubRequest, Response> {
    let v: serde_json::Value = serde_json::from_slice(body).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(MicropubError::new("invalid_request", "Invalid JSON body")),
        )
            .into_response()
    })?;

    match v.get("action").and_then(|a| a.as_str()) {
        None => parse_json_create(v),
        Some("update") => parse_json_update(v),
        Some("delete") => {
            let url = v
                .get("url")
                .and_then(|u| u.as_str())
                .ok_or_else(|| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(MicropubError::new("invalid_request", "url required for delete")),
                    )
                        .into_response()
                })?
                .to_string();
            Ok(MicropubRequest::Delete { url })
        }
        Some("undelete") => {
            let url = v
                .get("url")
                .and_then(|u| u.as_str())
                .ok_or_else(|| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(MicropubError::new("invalid_request", "url required for undelete")),
                    )
                        .into_response()
                })?
                .to_string();
            Ok(MicropubRequest::Undelete { url })
        }
        Some(a) => Err((
            StatusCode::BAD_REQUEST,
            Json(MicropubError::new(
                "invalid_request",
                &format!("Unknown action: {}", a),
            )),
        )
            .into_response()),
    }
}

fn parse_json_create(v: serde_json::Value) -> Result<MicropubRequest, Response> {
    // Validate h-entry type
    let type_arr = v
        .get("type")
        .and_then(|t| t.as_array())
        .cloned()
        .unwrap_or_default();
    let is_entry = type_arr
        .iter()
        .any(|t| t.as_str() == Some("h-entry"));
    if !type_arr.is_empty() && !is_entry {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(MicropubError::new("invalid_request", "Only h-entry is supported")),
        )
            .into_response());
    }

    let props = v
        .get("properties")
        .cloned()
        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

    let first_str = |key: &str| -> Option<String> {
        props
            .get(key)
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };

    let all_strs = |key: &str| -> Vec<String> {
        props
            .get(key)
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default()
    };

    // Content can be plain string or {"html":...} object
    let content = props
        .get("content")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .map(extract_content_value)
        .unwrap_or_default();

    Ok(MicropubRequest::Create(CreateEntry {
        name: first_str("name"),
        content,
        tags: all_strs("category"),
        slug: first_str("mp-slug"),
        published: first_str("published"),
    }))
}

fn parse_json_update(v: serde_json::Value) -> Result<MicropubRequest, Response> {
    let url = v
        .get("url")
        .and_then(|u| u.as_str())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(MicropubError::new("invalid_request", "url required for update")),
            )
                .into_response()
        })?
        .to_string();

    let parse_map = |key: &str| -> HashMap<String, Vec<serde_json::Value>> {
        v.get(key)
            .and_then(|m| m.as_object())
            .map(|obj| {
                obj.iter()
                    .map(|(k, val)| {
                        let values = val.as_array().cloned().unwrap_or_default();
                        (k.clone(), values)
                    })
                    .collect()
            })
            .unwrap_or_default()
    };

    // `delete` in an update can be an array of property names or an object
    let delete: Vec<String> = v
        .get("delete")
        .and_then(|d| d.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();

    Ok(MicropubRequest::Update(UpdateRequest {
        url,
        replace: parse_map("replace"),
        add: parse_map("add"),
        delete,
    }))
}

// ── Utilities ─────────────────────────────────────────────────────────────────

/// Convert a Micropub post URL (absolute or relative) to a relative file path
/// within www_root, including `.md` extension.
pub fn url_to_rel_path(state: &AppState, url: &str) -> Option<String> {
    // Strip base_url prefix if present
    let path = if let Some(base) = state.base_url.as_deref() {
        let base = base.trim_end_matches('/');
        if let Some(stripped) = url.strip_prefix(base) {
            stripped
        } else if url.starts_with('/') {
            url
        } else {
            return None;
        }
    } else if url.starts_with('/') {
        url
    } else {
        return None;
    };

    let rel = path.trim_start_matches('/');
    if rel.is_empty() {
        return None;
    }

    Some(if rel.ends_with(".md") {
        rel.to_string()
    } else {
        format!("{}.md", rel)
    })
}

/// Convert a title string into a URL-safe slug.
/// Lowercases, replaces non-alphanumeric chars with hyphens, collapses runs,
/// and trims to 60 characters at a word boundary.
pub fn slugify(title: &str) -> String {
    let slug: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();

    let parts: Vec<&str> = slug.split('-').filter(|s| !s.is_empty()).collect();
    let joined = parts.join("-");

    let truncated = if joined.len() > 60 {
        let cut = joined[..60].rfind('-').unwrap_or(60);
        joined[..cut].trim_end_matches('-').to_string()
    } else {
        joined
    };

    if truncated.is_empty() {
        "untitled".to_string()
    } else {
        truncated
    }
}

/// Normalise an ISO 8601 date string to `YYYY-MM-DD`.
fn normalize_date(s: &str) -> String {
    if s.len() >= 10 {
        let prefix = &s[..10];
        let chars: Vec<char> = prefix.chars().collect();
        if chars.len() == 10
            && chars[4] == '-'
            && chars[7] == '-'
            && chars[..4].iter().all(|c| c.is_ascii_digit())
            && chars[5..7].iter().all(|c| c.is_ascii_digit())
            && chars[8..].iter().all(|c| c.is_ascii_digit())
        {
            return prefix.to_string();
        }
    }
    s.to_string()
}
