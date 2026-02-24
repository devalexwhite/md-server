use axum::{
    body::Body,
    extract::State,
    http::{StatusCode, Uri, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use std::{io, path::Path};
use tokio_util::io::ReaderStream;

use crate::{
    css::find_css,
    error::AppError,
    front_matter::{self, ParsedDoc},
    state::AppState,
    template::{self, DirEntry},
};

/// File extensions served as static pass-throughs (not converted to HTML).
const STATIC_EXTENSIONS: &[&str] = &[
    "css", "js", "mjs", "png", "jpg", "jpeg", "gif", "svg", "webp", "ico", "avif", "woff", "woff2",
    "ttf", "otf", "eot", "txt", "pdf", "mp4", "webm", "mp3", "ogg", "wav",
];

pub async fn handle(State(state): State<AppState>, uri: Uri) -> Result<Response, AppError> {
    let raw_path = uri.path();

    // Decode percent-encoded characters; reject if the path is not valid UTF-8.
    let decoded = percent_decode(raw_path).ok_or(AppError::NotFound)?;

    // Reject path traversal attempts early.
    if decoded.split('/').any(|seg| seg == "..") {
        return Err(AppError::NotFound);
    }

    let rel = decoded.trim_start_matches('/');
    let fs_path = state.www_root.join(rel);

    // Fast lexical guard — validate_path() performs canonicalize for the real check.
    if !fs_path.starts_with(&state.www_root) {
        return Err(AppError::NotFound);
    }

    // Root or trailing slash → directory listing.
    if raw_path.ends_with('/') || rel.is_empty() {
        return serve_directory(&state, &fs_path, &decoded).await;
    }

    // /any/path/index.html → treat as its parent directory.
    if raw_path.ends_with("/index.html") {
        let dir_url = decoded.strip_suffix("index.html").unwrap_or("/");
        let dir_fs = state.www_root.join(dir_url.trim_start_matches('/'));
        return serve_directory(&state, &dir_fs, dir_url).await;
    }

    // Real directory on disk without trailing slash → redirect to canonical URL.
    if tokio::fs::metadata(&fs_path)
        .await
        .map(|m| m.is_dir())
        .unwrap_or(false)
    {
        return Ok(Redirect::permanent(&format!("{}/", raw_path)).into_response());
    }

    let ext = file_extension(&fs_path);

    match ext.as_deref() {
        Some("md") => serve_markdown(&state, &fs_path, &decoded).await,
        Some(e) if STATIC_EXTENSIONS.contains(&e) => serve_static(&state, &fs_path).await,
        _ => {
            // No or unrecognized extension — try appending .md for clean URLs.
            let md_path = fs_path.with_extension("md");
            if tokio::fs::try_exists(&md_path)
                .await
                .map_err(AppError::Io)?
            {
                serve_markdown(&state, &md_path, &decoded).await
            } else {
                Err(AppError::NotFound)
            }
        }
    }
}

async fn serve_markdown(
    state: &AppState,
    fs_path: &Path,
    url_path: &str,
) -> Result<Response, AppError> {
    let real_path = validate_path(state, fs_path).await?;
    let raw = tokio::fs::read_to_string(&real_path)
        .await
        .map_err(io_err)?;

    let ParsedDoc {
        mut front_matter,
        content,
    } = front_matter::parse(&raw);
    front_matter::fill_inferred(&mut front_matter, &content, &real_path).await;

    let html_body = render_markdown(&content);
    let css = find_css(&state.canonical_root, &real_path).await;
    let breadcrumbs = template::build_breadcrumbs(url_path);
    let markup = template::page(&front_matter, &html_body, css.as_deref(), &breadcrumbs);

    Ok(Html(markup.into_string()).into_response())
}

async fn serve_directory(
    state: &AppState,
    fs_path: &Path,
    url_path: &str,
) -> Result<Response, AppError> {
    let real_path = validate_path(state, fs_path).await?;

    // Prefer index.md if present.
    let index_md = real_path.join("index.md");
    if tokio::fs::try_exists(&index_md).await.unwrap_or(false) {
        return serve_markdown(state, &index_md, url_path).await;
    }

    let mut read_dir = tokio::fs::read_dir(&real_path).await.map_err(io_err)?;
    let mut entries: Vec<DirEntry> = Vec::new();
    let base_url = url_path.trim_end_matches('/');

    while let Some(entry) = read_dir.next_entry().await.map_err(AppError::Io)? {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }

        let entry_path = entry.path();
        let file_type = match entry.file_type().await {
            Ok(ft) => ft,
            Err(e) => {
                tracing::warn!("Cannot stat {}: {}", entry_path.display(), e);
                continue;
            }
        };

        if file_type.is_dir() {
            let dir_url = format!("{}/{}/", base_url, name);
            let date = front_matter::infer_date(&entry_path).await;
            let (title, summary, author) = read_index_metadata(&entry_path).await;
            entries.push(DirEntry {
                display_name: name,
                url: dir_url,
                is_dir: true,
                title,
                date,
                summary,
                author,
            });
        } else if file_type.is_file() {
            let Some(stem) = md_stem(&name) else {
                continue;
            };
            if stem == "index" {
                continue;
            }

            let file_url = format!("{}/{}", base_url, stem);
            let raw = tokio::fs::read_to_string(&entry_path)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!("Cannot read {}: {}", entry_path.display(), e);
                    String::new()
                });

            let ParsedDoc {
                mut front_matter,
                content,
            } = front_matter::parse(&raw);
            front_matter::fill_inferred(&mut front_matter, &content, &entry_path).await;

            entries.push(DirEntry {
                display_name: stem.to_string(),
                url: file_url,
                is_dir: false,
                title: front_matter.title,
                date: front_matter.date,
                summary: front_matter.summary,
                author: front_matter.author,
            });
        }
    }

    // Sort by date descending; undated entries last, sorted alphabetically.
    entries.sort_unstable_by(|a, b| match (&b.date, &a.date) {
        (Some(bd), Some(ad)) => bd.cmp(ad),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.display_name.cmp(&b.display_name),
    });

    let display_path = if url_path.is_empty() { "/" } else { url_path };
    let css = find_css(&state.canonical_root, &real_path).await;
    let markup = template::directory_index(display_path, &entries, css.as_deref());

    Ok(Html(markup.into_string()).into_response())
}

async fn serve_static(state: &AppState, fs_path: &Path) -> Result<Response, AppError> {
    let real_path = validate_path(state, fs_path).await?;

    let file = tokio::fs::File::open(&real_path).await.map_err(io_err)?;
    let content_length = file.metadata().await.map_err(AppError::Io)?.len();

    let mime: &'static str = mime_guess::from_path(&real_path)
        .first_raw()
        .unwrap_or("application/octet-stream");

    let body = Body::from_stream(ReaderStream::new(file));

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, content_length)
        .body(body)
        .map_err(|e| AppError::Internal(e.to_string()))
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Canonicalize `path` (resolving symlinks) and verify it stays within
/// `state.canonical_root`. Returns the resolved path on success.
async fn validate_path(state: &AppState, path: &Path) -> Result<std::path::PathBuf, AppError> {
    let canonical = tokio::fs::canonicalize(path).await.map_err(io_err)?;
    if !canonical.starts_with(&state.canonical_root) {
        return Err(AppError::NotFound);
    }
    Ok(canonical)
}

/// Map an `io::Error` to `AppError`, translating `NotFound` appropriately.
fn io_err(e: io::Error) -> AppError {
    if e.kind() == io::ErrorKind::NotFound {
        AppError::NotFound
    } else {
        AppError::Io(e)
    }
}

/// Return the stem of a `.md` filename, or `None` if it isn't a `.md` file.
fn md_stem(name: &str) -> Option<&str> {
    name.strip_suffix(".md")
}

/// Read `index.md` from `dir` and return (title, summary, author) for use in
/// directory listings. Date is not inferred here since directories use their
/// own modification time.
async fn read_index_metadata(dir: &Path) -> (Option<String>, Option<String>, Option<String>) {
    let raw = match tokio::fs::read_to_string(dir.join("index.md")).await {
        Ok(r) => r,
        Err(e) => {
            if e.kind() != io::ErrorKind::NotFound {
                tracing::warn!("Cannot read index.md in {}: {}", dir.display(), e);
            }
            return (None, None, None);
        }
    };

    let ParsedDoc {
        mut front_matter,
        content,
    } = front_matter::parse(&raw);

    if front_matter.title.is_none() {
        front_matter.title = front_matter::infer_title(&content);
    }
    if front_matter.summary.is_none() {
        front_matter.summary = front_matter::infer_summary(&content);
    }

    (
        front_matter.title,
        front_matter.summary,
        front_matter.author,
    )
}

fn render_markdown(content: &str) -> String {
    let mut opts = markdown::Options::gfm();
    opts.parse.constructs.frontmatter = false;
    opts.compile.allow_dangerous_html = true;
    markdown::to_html_with_options(content, &opts).unwrap_or_else(|_| markdown::to_html(content))
}

fn file_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
}

/// Percent-decode a URL path. Returns `None` if the decoded bytes are not
/// valid UTF-8 (which maps to a 404).
fn percent_decode(s: &str) -> Option<String> {
    percent_encoding::percent_decode_str(s)
        .decode_utf8()
        .ok()
        .map(|c| c.into_owned())
}
