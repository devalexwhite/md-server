use maud::{html, Markup, PreEscaped, DOCTYPE};
use super::handlers::urlencoded;

/// A node in the www-root file tree.
#[allow(dead_code)]
pub enum FileNode {
    Dir {
        name: String,
        rel: String,
        children: Vec<FileNode>,
    },
    File {
        name: String,
        rel: String,
    },
}

// ── Shared page shell ─────────────────────────────────────────────────────────

fn shell(title: &str, extra_head: Markup, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " — md-server editor" }
                (extra_head)
                style {
                    (PreEscaped(BASE_CSS))
                }
            }
            body {
                (body)
            }
        }
    }
}

// ── Login page ────────────────────────────────────────────────────────────────

pub fn login_page(error: Option<&str>) -> Markup {
    shell(
        "Login",
        html! {},
        html! {
            div class="login-wrap" {
                h1 { "md-server" }
                p class="login-sub" { "Editor dashboard" }
                form method="post" action="/edit/login" class="login-form" {
                    @if let Some(err) = error {
                        p class="error" { (err) }
                    }
                    label for="username" { "Username" }
                    input type="text" id="username" name="username" autocomplete="username" autofocus required;
                    label for="password" { "Password" }
                    input type="password" id="password" name="password" autocomplete="current-password" required;
                    button type="submit" { "Sign in" }
                }
            }
        },
    )
}

// ── Dashboard ─────────────────────────────────────────────────────────────────

pub fn dashboard(tree: &[FileNode]) -> Markup {
    shell(
        "Dashboard",
        htmx_head(),
        html! {
            div class="layout" {
                (sidebar(tree, None))
                main class="main-content" {
                    div class="welcome" {
                        h2 { "Welcome to the editor" }
                        p { "Select a file from the sidebar to edit it, or create a new one below." }
                        (new_file_form())
                        (new_dir_form())
                    }
                }
            }
        },
    )
}

// ── Editor page ───────────────────────────────────────────────────────────────

pub fn editor_page(rel_path: &str, content: &str, tree: &[FileNode]) -> Markup {
    shell(
        rel_path,
        html! {
            (htmx_head())
            (codemirror_head())
        },
        html! {
            div class="layout" {
                (sidebar(tree, Some(rel_path)))
                main class="main-content editor-main" {
                    div class="editor-toolbar" {
                        span class="editor-path" { (rel_path) }
                        span id="save-status" class="save-status" {}
                        (rename_form(rel_path))
                    }
                    div class="editor-panes" {
                        div class="pane pane-editor" {
                            form id="editor-form" {
                                input type="hidden" name="path" value=(rel_path);
                                textarea
                                    id="editor-content"
                                    name="content"
                                    hx-post="/edit/save"
                                    hx-trigger="keyup changed delay:1500ms, save-shortcut"
                                    hx-target="#save-status"
                                    hx-swap="outerHTML"
                                { (content) }
                            }
                        }
                        div class="pane pane-preview" {
                            div id="preview"
                                hx-post="/edit/preview"
                                hx-trigger="load, editor-change delay:800ms from:#editor-content"
                                hx-vals="js:{content: getEditorContent()}"
                                hx-target="#preview"
                                hx-swap="innerHTML"
                            {
                                em { "Loading preview…" }
                            }
                        }
                    }
                    // Delete button
                    @let delete_url = format!("/edit/delete?path={}", urlencoded(rel_path));
                    details class="danger-zone" {
                        summary { "Danger zone" }
                        form
                            hx-delete=(delete_url)
                            hx-confirm={"Delete "" (rel_path) ""? This cannot be undone."}
                            hx-push-url="/edit"
                        {
                            button type="submit" class="btn-danger" { "Delete this file" }
                        }
                    }
                }
            }
            (codemirror_init())
        },
    )
}

// ── Sidebar ───────────────────────────────────────────────────────────────────

fn sidebar(tree: &[FileNode], active: Option<&str>) -> Markup {
    html! {
        aside class="sidebar" {
            div class="sidebar-header" {
                a href="/edit" class="brand" { "md-server" }
                form method="post" action="/edit/logout" style="display:inline" {
                    button type="submit" class="btn-link" title="Sign out" { "Sign out" }
                }
            }
            nav class="file-tree" {
                (render_tree(tree, active))
            }
            div class="sidebar-footer" {
                a href="/" target="_blank" { "View site ↗" }
            }
        }
    }
}

fn render_tree(nodes: &[FileNode], active: Option<&str>) -> Markup {
    html! {
        ul {
            @for node in nodes {
                @match node {
                    FileNode::Dir { name, rel: _, children } => {
                        li class="tree-dir" {
                            details open {
                                summary { "📁 " (name) }
                                (render_tree(children, active))
                            }
                        }
                    }
                    FileNode::File { name, rel } => {
                        @let is_active = active == Some(rel.as_str());
                        @let href = format!("/edit/open?path={}", urlencoded(rel));
                        li class=(if is_active { "tree-file active" } else { "tree-file" }) {
                            a href=(href) { (name) }
                        }
                    }
                }
            }
        }
    }
}

// ── New file / dir forms ──────────────────────────────────────────────────────

fn new_file_form() -> Markup {
    html! {
        form method="post" action="/edit/new-file" class="inline-form" {
            input type="text" name="path" placeholder="path/to/new-post.md" required;
            button type="submit" { "New file" }
        }
    }
}

fn new_dir_form() -> Markup {
    html! {
        form method="post" action="/edit/new-dir" class="inline-form" {
            input type="text" name="path" placeholder="path/to/new-folder" required;
            button type="submit" { "New folder" }
        }
    }
}

// ── Rename form ───────────────────────────────────────────────────────────────

fn rename_form(rel_path: &str) -> Markup {
    html! {
        details class="rename-details" {
            summary class="btn-link" { "Rename / move" }
            form method="post" action="/edit/rename" class="inline-form" {
                input type="hidden" name="old_path" value=(rel_path);
                input type="text" name="new_path" value=(rel_path) required;
                button type="submit" { "Rename" }
            }
        }
    }
}

// ── CDN script / link tags ────────────────────────────────────────────────────

fn htmx_head() -> Markup {
    html! {
        script src="https://unpkg.com/htmx.org@2.0.4/dist/htmx.min.js" {}
    }
}

fn codemirror_head() -> Markup {
    html! {
        link rel="stylesheet" href="https://unpkg.com/codemirror@5.65.17/lib/codemirror.css";
        script src="https://unpkg.com/codemirror@5.65.17/lib/codemirror.js" {}
        script src="https://unpkg.com/codemirror@5.65.17/mode/markdown/markdown.js" {}
        script src="https://unpkg.com/codemirror@5.65.17/addon/edit/continuelist.js" {}
    }
}

fn codemirror_init() -> Markup {
    html! {
        script { (PreEscaped(r#"
(function () {
  var ta = document.getElementById('editor-content');
  if (!ta) return;

  var cm = CodeMirror.fromTextArea(ta, {
    mode: 'markdown',
    lineNumbers: true,
    lineWrapping: true,
    indentUnit: 2,
    tabSize: 2,
    extraKeys: {
      'Enter': 'newlineAndIndentContinueMarkdownList',
      'Ctrl-S': function(cm) {
        cm.save();
        htmx.trigger(ta, 'save-shortcut');
      },
      'Cmd-S': function(cm) {
        cm.save();
        htmx.trigger(ta, 'save-shortcut');
      }
    }
  });

  // Keep textarea in sync and fire HTMX events.
  cm.on('change', function () {
    cm.save();
    htmx.trigger(ta, 'editor-change');
  });

  // Expose current content for hx-vals.
  window.getEditorContent = function () {
    return cm.getValue();
  };
})();
"#)) }
    }
}

// ── CSS ───────────────────────────────────────────────────────────────────────

const BASE_CSS: &str = r#"
*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

:root {
  --bg: #f8f8f8;
  --surface: #ffffff;
  --border: #e0e0e0;
  --text: #1a1a1a;
  --muted: #666;
  --accent: #2563eb;
  --accent-hover: #1d4ed8;
  --danger: #dc2626;
  --sidebar-w: 240px;
  --toolbar-h: 44px;
  font-size: 15px;
}

body { font-family: system-ui, sans-serif; background: var(--bg); color: var(--text); height: 100vh; overflow: hidden; }

a { color: var(--accent); text-decoration: none; }
a:hover { text-decoration: underline; }

/* ── Login ── */
.login-wrap {
  max-width: 360px; margin: 80px auto; padding: 2rem;
  background: var(--surface); border: 1px solid var(--border); border-radius: 8px;
}
.login-wrap h1 { font-size: 1.4rem; margin-bottom: .25rem; }
.login-sub { color: var(--muted); margin-bottom: 1.5rem; font-size: .9rem; }
.login-form { display: flex; flex-direction: column; gap: .75rem; }
.login-form label { font-size: .85rem; font-weight: 600; }
.login-form input {
  padding: .5rem .75rem; border: 1px solid var(--border); border-radius: 6px;
  font-size: 1rem; outline: none;
}
.login-form input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px #2563eb22; }
.login-form button {
  padding: .6rem; background: var(--accent); color: #fff; border: none;
  border-radius: 6px; font-size: 1rem; cursor: pointer; font-weight: 600;
}
.login-form button:hover { background: var(--accent-hover); }
.error { color: var(--danger); font-size: .875rem; }

/* ── Layout ── */
.layout { display: flex; height: 100vh; overflow: hidden; }

/* ── Sidebar ── */
.sidebar {
  width: var(--sidebar-w); flex-shrink: 0; background: var(--surface);
  border-right: 1px solid var(--border); display: flex; flex-direction: column;
  overflow: hidden;
}
.sidebar-header {
  padding: .75rem 1rem; border-bottom: 1px solid var(--border);
  display: flex; align-items: center; justify-content: space-between;
}
.brand { font-weight: 700; font-size: 1rem; color: var(--text); }
.file-tree { flex: 1; overflow-y: auto; padding: .5rem 0; font-size: .875rem; }
.file-tree ul { list-style: none; padding-left: 1rem; }
.file-tree > ul { padding-left: .5rem; }
.tree-file a { display: block; padding: .25rem .5rem; border-radius: 4px; color: var(--text); }
.tree-file a:hover, .tree-file.active a {
  background: #2563eb18; color: var(--accent); text-decoration: none;
}
.tree-dir summary { padding: .3rem .5rem; cursor: pointer; list-style: none; font-weight: 600; }
.tree-dir summary::-webkit-details-marker { display: none; }
.sidebar-footer { padding: .75rem 1rem; border-top: 1px solid var(--border); font-size: .8rem; }

/* ── Main content ── */
.main-content { flex: 1; display: flex; flex-direction: column; overflow: hidden; }
.welcome { padding: 2rem; }
.welcome h2 { margin-bottom: .5rem; }
.welcome p { color: var(--muted); margin-bottom: 1.5rem; }

/* ── Editor layout ── */
.editor-main { display: flex; flex-direction: column; }
.editor-toolbar {
  height: var(--toolbar-h); padding: 0 1rem; border-bottom: 1px solid var(--border);
  display: flex; align-items: center; gap: 1rem; background: var(--surface);
  flex-shrink: 0;
}
.editor-path { font-size: .875rem; color: var(--muted); font-family: monospace; flex: 1; }
.save-status { font-size: .8rem; color: #16a34a; }
.editor-panes { display: flex; flex: 1; overflow: hidden; }
.pane { flex: 1; overflow: auto; }
.pane-editor { border-right: 1px solid var(--border); display: flex; flex-direction: column; }
.pane-editor .CodeMirror {
  flex: 1; height: 100%; font-family: 'JetBrains Mono', 'Fira Code', monospace; font-size: .875rem; line-height: 1.6;
}
.pane-preview { padding: 1.5rem 2rem; font-size: .9375rem; line-height: 1.7; overflow-y: auto; }
.pane-preview h1,h2,h3,h4,h5,h6 { margin: 1.25em 0 .5em; }
.pane-preview p { margin: .75em 0; }
.pane-preview pre { background: var(--bg); padding: 1em; border-radius: 6px; overflow-x: auto; }
.pane-preview code { font-family: monospace; font-size: .875em; background: var(--bg); padding: .1em .3em; border-radius: 3px; }
.pane-preview pre code { background: none; padding: 0; }
.pane-preview blockquote { border-left: 3px solid var(--border); padding-left: 1em; color: var(--muted); }

/* ── Forms ── */
.inline-form { display: flex; gap: .5rem; margin-bottom: .75rem; }
.inline-form input[type=text] {
  flex: 1; padding: .4rem .6rem; border: 1px solid var(--border);
  border-radius: 6px; font-size: .875rem;
}
.inline-form input:focus { outline: none; border-color: var(--accent); }
.inline-form button {
  padding: .4rem .8rem; background: var(--accent); color: #fff;
  border: none; border-radius: 6px; cursor: pointer; font-size: .875rem; white-space: nowrap;
}
.inline-form button:hover { background: var(--accent-hover); }
.btn-link { background: none; border: none; color: var(--accent); cursor: pointer; font-size: .875rem; padding: 0; }
.btn-link:hover { text-decoration: underline; }

/* ── Danger zone ── */
.danger-zone { padding: 1rem; border-top: 1px solid var(--border); }
.danger-zone summary { color: var(--muted); font-size: .875rem; cursor: pointer; }
.btn-danger {
  margin-top: .75rem; padding: .4rem .8rem; background: var(--danger);
  color: #fff; border: none; border-radius: 6px; cursor: pointer; font-size: .875rem;
}
.btn-danger:hover { background: #b91c1c; }

/* ── Rename ── */
.rename-details { display: inline; }
.rename-details summary { font-size: .8rem; }

/* ── Misc ── */
.save-ok { color: #16a34a; font-size: .8rem; }
"#;
