use chrono::{DateTime, Local};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize, Default, Debug, Clone)]
pub struct FrontMatter {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub author: Option<String>,
    pub date: Option<String>,
}

pub struct ParsedDoc {
    pub front_matter: FrontMatter,
    pub content: String,
}

/// Strip and parse YAML front matter delimited by `---`.
/// Returns the parsed metadata and the remaining markdown content.
pub fn parse(raw: &str) -> ParsedDoc {
    // Strip UTF-8 BOM if present.
    let text = raw.strip_prefix('\u{feff}').unwrap_or(raw);

    // Must start with "---\n" or "---\r\n".
    let rest = match text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    {
        Some(r) => r,
        None => {
            return ParsedDoc {
                front_matter: FrontMatter::default(),
                content: text.to_string(),
            }
        }
    };

    // Find the closing "---" on its own line (followed by \n, \r\n, or end-of-string).
    let Some((yaml_end, content_start)) = find_close_delimiter(rest) else {
        return ParsedDoc {
            front_matter: FrontMatter::default(),
            content: text.to_string(),
        };
    };

    let yaml_str = &rest[..yaml_end];
    let content = &rest[content_start..];
    let fm: FrontMatter = serde_yml::from_str(yaml_str).unwrap_or_default();

    ParsedDoc {
        front_matter: fm,
        content: content.to_string(),
    }
}

/// Find `\n---` that is immediately followed by `\n`, `\r\n`, or end-of-string.
/// Returns `(yaml_end, content_start)`:
/// - `yaml_end`: position of the `\n` before `---` (end of YAML text)
/// - `content_start`: position where the markdown content begins
fn find_close_delimiter(s: &str) -> Option<(usize, usize)> {
    let mut search = 0;
    while let Some(rel) = s[search..].find("\n---") {
        let pos = search + rel;
        let after = pos + 4; // position right after "\n---"

        if after >= s.len() {
            // "\n---" at end of string
            return Some((pos, after));
        }

        let next = &s[after..];
        if next.starts_with('\n') {
            return Some((pos, after + 1));
        }
        if next.starts_with("\r\n") {
            return Some((pos, after + 2));
        }

        // Not a valid close (e.g. "---more-text") — keep searching.
        search = pos + 1;
    }
    None
}

/// Fill in any missing front matter fields by inference from content and file metadata.
pub async fn fill_inferred(fm: &mut FrontMatter, content: &str, path: &Path) {
    if fm.title.is_none() {
        fm.title = infer_title(content);
    }
    if fm.summary.is_none() {
        fm.summary = infer_summary(content);
    }
    if fm.date.is_none() {
        fm.date = infer_date(path).await;
    }
}

/// Infer a title from the first `# Heading` in the content.
pub fn infer_title(content: &str) -> Option<String> {
    for line in content.lines() {
        if let Some(rest) = line.trim().strip_prefix("# ") {
            let title = rest.trim().to_string();
            if !title.is_empty() {
                return Some(title);
            }
        }
    }
    None
}

/// Infer a summary from the first non-heading paragraph.
///
/// Handles ATX headings (`# Heading`) and Setext headings (`Title\n====`).
pub fn infer_summary(content: &str) -> Option<String> {
    let mut lines: Vec<&str> = Vec::new();
    let mut in_para = false;
    let mut in_code_block = false;
    let mut prev_was_text = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Track fenced code blocks.
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            if in_para {
                break;
            }
            prev_was_text = false;
            continue;
        }
        if in_code_block {
            continue;
        }

        // A Setext underline is a non-empty line of all `=` or `-` that follows text.
        let is_setext_underline = prev_was_text
            && !trimmed.is_empty()
            && trimmed.chars().all(|c| c == '=' || c == '-');

        if trimmed.is_empty() {
            if in_para {
                break;
            }
            prev_was_text = false;
        } else if trimmed.starts_with('#') {
            // ATX heading: stop collecting if already in a paragraph.
            if in_para {
                break;
            }
            prev_was_text = false;
        } else if is_setext_underline {
            // Setext heading underline: the lines collected so far were a heading
            // title, not a paragraph — discard them and start fresh.
            lines.clear();
            in_para = false;
            prev_was_text = false;
        } else {
            in_para = true;
            lines.push(trimmed);
            prev_was_text = true;
        }
    }

    if lines.is_empty() {
        None
    } else {
        Some(lines.join(" "))
    }
}

/// Infer a date string (YYYY-MM-DD) from file creation or modification time.
pub async fn infer_date(path: &Path) -> Option<String> {
    let meta = tokio::fs::metadata(path).await.ok()?;
    let sys_time = match meta.created() {
        Ok(t) => t,
        Err(_) => {
            // File creation time is unavailable on most Linux filesystems (e.g.
            // ext4, xfs). Fall back to modification time, which will be wrong
            // after a git clone or rsync.
            tracing::debug!(
                path = %path.display(),
                "file creation time unavailable, using mtime for date inference"
            );
            meta.modified().ok()?
        }
    };
    let dt: DateTime<Local> = sys_time.into();
    Some(dt.format("%Y-%m-%d").to_string())
}
