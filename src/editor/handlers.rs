use axum::{
    Form,
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use std::{
    io,
    path::{Path, PathBuf},
};

use crate::{
    error::AppError,
    state::AppState,
};

use super::template::{self, FileNode};

// ── Dashboard ─────────────────────────────────────────────────────────────────

pub async fn get_dashboard(State(state): State<AppState>) -> Response {
    match build_file_tree(&state.canonical_root, &state.canonical_root).await {
        Ok(tree) => Html(template::dashboard(&tree).into_string()).into_response(),
        Err(e) => AppError::Io(e).into_response(),
    }
}

// ── Editor page ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct PathParam {
    pub path: String,
}

pub async fn get_editor(
    State(state): State<AppState>,
    Query(params): Query<PathParam>,
) -> Response {
    let fs_path = match resolve_read_path(&state, &params.path).await {
        Ok(p) => p,
        Err(r) => return r,
    };

    let content = match tokio::fs::read_to_string(&fs_path).await {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return AppError::NotFound.into_response()
        }
        Err(e) => return AppError::Io(e).into_response(),
    };

    let tree = match build_file_tree(&state.canonical_root, &state.canonical_root).await {
        Ok(t) => t,
        Err(e) => return AppError::Io(e).into_response(),
    };

    Html(template::editor_page(&params.path, &content, &tree).into_string()).into_response()
}

// ── Save ──────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SaveForm {
    pub path: String,
    pub content: String,
}

pub async fn post_save(
    State(state): State<AppState>,
    Form(form): Form<SaveForm>,
) -> Response {
    let fs_path = match resolve_write_path(&state, &form.path).await {
        Ok(p) => p,
        Err(r) => return r,
    };

    if let Some(parent) = fs_path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return AppError::Io(e).into_response();
        }
    }

    if let Err(e) = tokio::fs::write(&fs_path, form.content.as_bytes()).await {
        return AppError::Io(e).into_response();
    }

    Html(r#"<span id="save-status" class="save-ok">Saved</span>"#.to_string()).into_response()
}

// ── Preview ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct PreviewForm {
    pub content: String,
}

pub async fn post_preview(
    State(_state): State<AppState>,
    Form(form): Form<PreviewForm>,
) -> Response {
    // Use safe rendering (no raw HTML passthrough) for the editor preview to
    // prevent XSS from user-controlled markdown content.
    let html = render_markdown_safe(&form.content);
    Html(html).into_response()
}

// ── New file ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct NewFileForm {
    /// Relative path under www root, e.g. "blog/new-post.md" or "blog/new-post"
    pub path: String,
}

pub async fn post_new_file(
    State(state): State<AppState>,
    Form(form): Form<NewFileForm>,
) -> Response {
    // Ensure .md extension.
    let path = if form.path.ends_with(".md") {
        form.path.clone()
    } else {
        format!("{}.md", form.path)
    };

    let fs_path = match resolve_write_path(&state, &path).await {
        Ok(p) => p,
        Err(r) => return r,
    };

    // Refuse to overwrite existing files.
    if tokio::fs::try_exists(&fs_path).await.unwrap_or(false) {
        return (
            StatusCode::CONFLICT,
            Html("<p class='error'>File already exists.</p>".to_string()),
        )
            .into_response();
    }

    if let Some(parent) = fs_path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return AppError::Io(e).into_response();
        }
    }

    if let Err(e) = tokio::fs::write(&fs_path, b"").await {
        return AppError::Io(e).into_response();
    }

    Redirect::to(&format!("/edit/open?path={}", urlencoded(&path))).into_response()
}

// ── New directory ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct NewDirForm {
    pub path: String,
}

pub async fn post_new_dir(
    State(state): State<AppState>,
    Form(form): Form<NewDirForm>,
) -> Response {
    let fs_path = match resolve_write_path(&state, &form.path).await {
        Ok(p) => p,
        Err(r) => return r,
    };

    if let Err(e) = tokio::fs::create_dir_all(&fs_path).await {
        return AppError::Io(e).into_response();
    }

    Redirect::to("/edit").into_response()
}

// ── Delete ────────────────────────────────────────────────────────────────────

pub async fn delete_file(
    State(state): State<AppState>,
    Query(params): Query<PathParam>,
) -> Response {
    let fs_path = match resolve_read_path(&state, &params.path).await {
        Ok(p) => p,
        Err(r) => return r,
    };

    let meta = match tokio::fs::metadata(&fs_path).await {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return AppError::NotFound.into_response()
        }
        Err(e) => return AppError::Io(e).into_response(),
    };

    let result = if meta.is_dir() {
        tokio::fs::remove_dir_all(&fs_path).await
    } else {
        tokio::fs::remove_file(&fs_path).await
    };

    if let Err(e) = result {
        return AppError::Io(e).into_response();
    }

    Redirect::to("/edit").into_response()
}

// ── Rename / move ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RenameForm {
    pub old_path: String,
    pub new_path: String,
}

pub async fn post_rename(
    State(state): State<AppState>,
    Form(form): Form<RenameForm>,
) -> Response {
    let src = match resolve_read_path(&state, &form.old_path).await {
        Ok(p) => p,
        Err(r) => return r,
    };

    let dst = match resolve_write_path(&state, &form.new_path).await {
        Ok(p) => p,
        Err(r) => return r,
    };

    // Refuse to overwrite.
    if tokio::fs::try_exists(&dst).await.unwrap_or(false) {
        return (
            StatusCode::CONFLICT,
            Html("<p class='error'>Destination already exists.</p>".to_string()),
        )
            .into_response();
    }

    if let Some(parent) = dst.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return AppError::Io(e).into_response();
        }
    }

    if let Err(e) = tokio::fs::rename(&src, &dst).await {
        return AppError::Io(e).into_response();
    }

    Redirect::to("/edit").into_response()
}

// ── Path helpers ──────────────────────────────────────────────────────────────

/// Resolve a client-supplied relative path for **reading** (file must exist).
/// Calls `canonicalize` to block symlink escapes, matching `handler.rs::validate_path`.
async fn resolve_read_path(state: &AppState, rel: &str) -> Result<PathBuf, Response> {
    let rel = sanitize_rel(rel)?;
    let joined = state.canonical_root.join(&rel);

    // Lexical guard — fast reject before the syscall.
    if !joined.starts_with(&state.canonical_root) {
        return Err(AppError::NotFound.into_response());
    }

    // Canonical guard — resolves symlinks and verifies containment.
    let canonical = tokio::fs::canonicalize(&joined)
        .await
        .map_err(|e| io_err_to_response(e))?;
    if !canonical.starts_with(&state.canonical_root) {
        return Err(AppError::NotFound.into_response());
    }
    Ok(canonical)
}

/// Resolve a client-supplied relative path for **writing** (file may not exist).
/// Canonicalizes the **parent** directory to block symlink escapes, then
/// re-appends the sanitized filename.
async fn resolve_write_path(state: &AppState, rel: &str) -> Result<PathBuf, Response> {
    let rel = sanitize_rel(rel)?;
    let joined = state.canonical_root.join(&rel);

    // Lexical guard.
    if !joined.starts_with(&state.canonical_root) {
        return Err(AppError::NotFound.into_response());
    }

    let parent = joined.parent().ok_or_else(|| AppError::NotFound.into_response())?;
    let file_name = joined
        .file_name()
        .ok_or_else(|| AppError::NotFound.into_response())?
        .to_owned();

    // Create the parent directory if needed before canonicalizing.
    if !parent.exists() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| AppError::Io(e).into_response())?;
    }

    // Canonical guard on parent (file itself may not exist yet).
    let canonical_parent = tokio::fs::canonicalize(parent)
        .await
        .map_err(|e| io_err_to_response(e))?;
    if !canonical_parent.starts_with(&state.canonical_root) {
        return Err(AppError::NotFound.into_response());
    }

    Ok(canonical_parent.join(file_name))
}

/// Sanitize a client-supplied relative path:
/// - Strip leading slashes
/// - Reject any `..` segment
/// - Reject empty paths
fn sanitize_rel(rel: &str) -> Result<String, Response> {
    let stripped = rel.trim_start_matches('/');
    if stripped.is_empty() {
        return Err(AppError::NotFound.into_response());
    }
    if stripped.split('/').any(|seg| seg == "..") {
        return Err(AppError::NotFound.into_response());
    }
    Ok(stripped.to_string())
}

fn io_err_to_response(e: std::io::Error) -> Response {
    if e.kind() == io::ErrorKind::NotFound {
        AppError::NotFound.into_response()
    } else {
        AppError::Io(e).into_response()
    }
}

// ── File tree ─────────────────────────────────────────────────────────────────

/// Recursively build a file tree under `dir`, rooted at `root`.
/// Returns nodes sorted: directories first, then files, both alphabetically.
pub async fn build_file_tree(root: &Path, dir: &Path) -> io::Result<Vec<FileNode>> {
    let mut read_dir = tokio::fs::read_dir(dir).await?;
    let mut nodes: Vec<FileNode> = Vec::new();

    while let Some(entry) = read_dir.next_entry().await? {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }

        let path = entry.path();
        let ft = match entry.file_type().await {
            Ok(ft) => ft,
            Err(_) => continue,
        };

        // Relative path from www root for use in URLs / API calls.
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        if ft.is_dir() {
            let children = Box::pin(build_file_tree(root, &path)).await?;
            nodes.push(FileNode::Dir { name, rel, children });
        } else if ft.is_file() {
            nodes.push(FileNode::File { name, rel });
        }
    }

    // Directories first, then files; alphabetical within each group.
    nodes.sort_unstable_by(|a, b| match (a, b) {
        (FileNode::Dir { .. }, FileNode::File { .. }) => std::cmp::Ordering::Less,
        (FileNode::File { .. }, FileNode::Dir { .. }) => std::cmp::Ordering::Greater,
        (FileNode::Dir { name: na, .. }, FileNode::Dir { name: nb, .. }) => na.cmp(nb),
        (FileNode::File { name: na, .. }, FileNode::File { name: nb, .. }) => na.cmp(nb),
    });

    Ok(nodes)
}

// ── Markdown rendering ────────────────────────────────────────────────────────

/// Safe markdown rendering for the editor preview: raw HTML passthrough is
/// disabled to prevent XSS in the preview pane.
fn render_markdown_safe(content: &str) -> String {
    let mut opts = markdown::Options::gfm();
    opts.parse.constructs.frontmatter = false;
    opts.compile.allow_dangerous_html = false;
    markdown::to_html_with_options(content, &opts).unwrap_or_else(|_| markdown::to_html(content))
}

/// Percent-encode a path for safe use in URL query strings.
pub fn urlencoded(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}
