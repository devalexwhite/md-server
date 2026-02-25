use std::path::{Path, PathBuf};

/// Walk up the directory tree from `file_path` toward `www_root`, checking
/// each directory for a `style.css` file. Returns the first found as an
/// absolute URL path (e.g. `/a/b/style.css`).
pub async fn find_css(www_root: &Path, file_path: &Path) -> Option<String> {
    let start: PathBuf = if file_path.is_dir() {
        file_path.to_path_buf()
    } else {
        file_path.parent()?.to_path_buf()
    };

    // Only search within www_root.
    if !start.starts_with(www_root) {
        return None;
    }

    let mut dir = start;
    loop {
        let candidate = dir.join("style.css");
        if tokio::fs::try_exists(&candidate).await.unwrap_or(false) {
            let rel = candidate.strip_prefix(www_root).ok()?;
            return Some(format!("/{}", rel.to_string_lossy().replace('\\', "/")));
        }

        if dir == www_root {
            break;
        }

        match dir.parent() {
            Some(parent) if parent.starts_with(www_root) => {
                dir = parent.to_path_buf();
            }
            _ => break,
        }
    }

    None
}

/// Walk up the directory tree from `file_path` toward `www_root`, checking
/// each directory for a `meta.<ext>` image file. Returns the first found as an
/// absolute URL path (e.g. `/a/b/meta.png`).
pub async fn find_meta_image(www_root: &Path, file_path: &Path) -> Option<String> {
    const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "avif", "svg"];

    let start: PathBuf = if file_path.is_dir() {
        file_path.to_path_buf()
    } else {
        file_path.parent()?.to_path_buf()
    };

    if !start.starts_with(www_root) {
        return None;
    }

    let mut dir = start;
    loop {
        for ext in IMAGE_EXTS {
            let candidate = dir.join(format!("meta.{}", ext));
            if tokio::fs::try_exists(&candidate).await.unwrap_or(false) {
                let rel = candidate.strip_prefix(www_root).ok()?;
                return Some(format!("/{}", rel.to_string_lossy().replace('\\', "/")));
            }
        }

        if dir == www_root {
            break;
        }

        match dir.parent() {
            Some(parent) if parent.starts_with(www_root) => {
                dir = parent.to_path_buf();
            }
            _ => break,
        }
    }

    None
}
