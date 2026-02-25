use std::path::{Path, PathBuf};

/// Walk up the directory tree from `file_path` toward `www_root` and return
/// the sequence of directories to check, starting closest to the file.
fn ancestor_dirs(www_root: &Path, file_path: &Path) -> Vec<PathBuf> {
    let start: PathBuf = if file_path.is_dir() {
        file_path.to_path_buf()
    } else {
        match file_path.parent() {
            Some(p) => p.to_path_buf(),
            None => return Vec::new(),
        }
    };

    if !start.starts_with(www_root) {
        return Vec::new();
    }

    let mut dirs = Vec::new();
    let mut dir = start;
    loop {
        dirs.push(dir.clone());
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
    dirs
}

/// Walk up the directory tree from `file_path` toward `www_root`, checking
/// each directory for a `style.css` file. Returns the first found as an
/// absolute URL path (e.g. `/a/b/style.css`).
pub async fn find_css(www_root: &Path, file_path: &Path) -> Option<String> {
    for dir in ancestor_dirs(www_root, file_path) {
        let candidate = dir.join("style.css");
        if tokio::fs::try_exists(&candidate).await.unwrap_or(false) {
            let rel = candidate.strip_prefix(www_root).ok()?;
            return Some(format!("/{}", rel.to_string_lossy().replace('\\', "/")));
        }
    }
    None
}

/// Walk up the directory tree from `file_path` toward `www_root`, checking
/// each directory for a `meta.<ext>` image file. Returns the first found as an
/// absolute URL path (e.g. `/a/b/meta.png`).
pub async fn find_meta_image(www_root: &Path, file_path: &Path) -> Option<String> {
    const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "avif", "svg"];

    for dir in ancestor_dirs(www_root, file_path) {
        for ext in IMAGE_EXTS {
            let candidate = dir.join(format!("meta.{}", ext));
            if tokio::fs::try_exists(&candidate).await.unwrap_or(false) {
                let rel = candidate.strip_prefix(www_root).ok()?;
                return Some(format!("/{}", rel.to_string_lossy().replace('\\', "/")));
            }
        }
    }
    None
}
