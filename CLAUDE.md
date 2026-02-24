# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build                          # compile (debug)
cargo build --release                # compile (release)
cargo run -- --root ./www            # run against a local www/ directory
cargo run -- --port 8080 --root ./www
RUST_LOG=debug cargo run -- --root ./www   # verbose logging
cargo check                          # fast type-check without linking
```

There are no tests yet. `cargo test` will compile but find nothing to run.

## Architecture

A single Axum catch-all handler routes every request. All request handling flows through `handler::handle` → one of three serving functions:

- **`serve_markdown`** — reads a `.md` file, parses YAML front matter, infers missing fields, renders to HTML via the `markdown` crate (GFM mode), wraps in a Maud template.
- **`serve_directory`** — if `index.md` exists, delegates to `serve_markdown`; otherwise reads the directory, collects metadata from each `.md` file, and renders a listing template.
- **`serve_static`** — streams the file via `ReaderStream` with `Content-Length` and a MIME type from `mime_guess`.

### Path resolution and security

`AppState` holds two roots: `www_root` (lexical) and `canonical_root` (symlink-resolved at startup). Before any file read, `validate_path()` calls `tokio::fs::canonicalize` on the resolved filesystem path and checks it stays within `canonical_root`. This blocks both `..` traversal and symlink escapes.

URL percent-decoding uses the `percent-encoding` crate; invalid UTF-8 after decoding returns 404.

### Front matter (`src/front_matter.rs`)

Parses `---\n…\n---\n` YAML blocks using `serde_yml`. The closing delimiter must be `---` on its own line (not `---more-text`). `fill_inferred()` fills any missing fields: title from the first `# H1`, summary from the first paragraph (handles both ATX and Setext headings), date from file ctime/mtime via `chrono`.

### CSS cascade (`src/css.rs`)

`find_css(www_root, file_path)` walks up from the served file's directory toward `www_root`, returning the first `style.css` found as an absolute URL path (e.g. `/blog/style.css`). Both arguments must be canonical paths.

### Templates (`src/template.rs`)

Two Maud functions: `page()` for markdown content (includes og:/article: meta tags from front matter) and `directory_index()` for listings. HTML responses use `axum::response::Html` — the maud `axum` feature is intentionally not used to avoid version coupling.

### Key routing rules

| Request pattern | Behaviour |
|---|---|
| `/dir/` or `/` | Directory listing (or `index.md` if present) |
| `/dir/index.html` | Same as `/dir/` |
| `/dir` (no slash, is a directory) | 308 redirect to `/dir/` |
| `/post.md` | Render markdown |
| `/post` (no extension) | Try `www/post.md` |
| Static extensions (css/js/png/jpg/gif/svg/webp/pdf/mp4 etc.) | Stream through |

## Configuration

| Flag | Env var | Default |
|---|---|---|
| `--port` | `PORT` | `3000` |
| `--host` | `HOST` | `0.0.0.0` |
| `--root` | `WWW_ROOT` | `www/` next to the binary |

Log level is controlled by `RUST_LOG` (e.g. `RUST_LOG=md_server=debug,tower_http=debug`).
