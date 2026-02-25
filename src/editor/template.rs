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

// ── Shared page shell ──────────────────────────────────────────────────────────

fn shell(title: &str, extra_head: Markup, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " — md-server" }
                link rel="preconnect" href="https://fonts.googleapis.com";
                link rel="preconnect" href="https://fonts.gstatic.com" crossorigin="anonymous";
                link rel="stylesheet"
                    href="https://fonts.googleapis.com/css2?family=Syne:wght@400;600;700;800&family=Lora:ital,wght@0,400;0,500;1,400&family=JetBrains+Mono:wght@400;500&display=swap";
                (extra_head)
                style { (PreEscaped(BASE_CSS)) }
            }
            body {
                div class="sidebar-backdrop" {}
                (context_menu_el())
                (modal_overlay_el())
                (hidden_forms_el())
                (body)
                script { (PreEscaped(UI_JS)) }
            }
        }
    }
}

fn context_menu_el() -> Markup {
    html! {
        div id="context-menu" class="context-menu" {
            div class="ctx-item" data-action="open" data-ctx-for="file" {
                span class="ctx-icon" { "↗" }
                "Open"
            }
            div class="ctx-item" data-action="new-file" data-ctx-for="dir" {
                span class="ctx-icon" { "+" }
                "New file here"
            }
            div class="ctx-item" data-action="new-folder" data-ctx-for="dir" {
                span class="ctx-icon" { "+" }
                "New folder here"
            }
            div class="ctx-separator" {}
            div class="ctx-item" data-action="rename" data-ctx-for="file,dir" {
                span class="ctx-icon" { "✎" }
                "Rename / move"
            }
            div class="ctx-item danger" data-action="delete" data-ctx-for="file,dir" {
                span class="ctx-icon" { "✕" }
                "Delete"
            }
        }
    }
}

fn modal_overlay_el() -> Markup {
    html! {
        div id="modal-overlay" class="modal-overlay" {
            div class="modal" {
                p id="modal-title" class="modal-title" {}
                input id="modal-input" class="modal-input" type="text";
                div class="modal-actions" {
                    button id="modal-cancel" class="btn-secondary" type="button" { "Cancel" }
                    button id="modal-confirm" class="btn-primary" type="button" { "Confirm" }
                }
            }
        }
    }
}

fn hidden_forms_el() -> Markup {
    html! {
        form id="hidden-new-file-form" method="post" action="/edit/new-file" style="display:none" {
            input type="hidden" name="path" value="";
        }
        form id="hidden-new-dir-form" method="post" action="/edit/new-dir" style="display:none" {
            input type="hidden" name="path" value="";
        }
        form id="hidden-rename-form" method="post" action="/edit/rename" style="display:none" {
            input type="hidden" name="old_path" value="";
            input type="hidden" name="new_path" value="";
        }
    }
}

// ── Login page ─────────────────────────────────────────────────────────────────

pub fn login_page(error: Option<&str>) -> Markup {
    shell(
        "Login",
        html! {},
        html! {
            div class="login-wrap" {
                div class="login-logo" { "md" span { "·" } "server" }
                p class="login-sub" { "Editor dashboard" }
                form method="post" action="/edit/login" class="login-form" {
                    @if let Some(err) = error {
                        p class="error" { (err) }
                    }
                    div class="form-group" {
                        label for="username" { "Username" }
                        input type="text" id="username" name="username"
                            autocomplete="username" autofocus required;
                    }
                    div class="form-group" {
                        label for="password" { "Password" }
                        input type="password" id="password" name="password"
                            autocomplete="current-password" required;
                    }
                    button type="submit" { "Sign in" }
                }
            }
        },
    )
}

// ── Dashboard ──────────────────────────────────────────────────────────────────

pub fn dashboard(tree: &[FileNode]) -> Markup {
    shell(
        "Dashboard",
        htmx_head(),
        html! {
            div class="layout" {
                (sidebar(tree, None))
                main class="main-content" {
                    div class="page-topbar" {
                        button id="sidebar-toggle" class="hamburger" type="button" aria-label="Toggle sidebar" {
                            (PreEscaped(HAMBURGER_SVG))
                        }
                        span class="topbar-title" { "md·server" }
                    }
                    div class="dashboard" {
                        div class="dashboard-header" {
                            h2 class="dashboard-title" { "Your content" }
                            p class="dashboard-subtitle" {
                                "Select a file from the sidebar to begin editing."
                            }
                        }
                        div class="action-cards" {
                            button class="action-card" type="button" onclick="showNewFileModal()" {
                                div class="action-card-icon" {
                                    (PreEscaped(r#"<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="12" y1="18" x2="12" y2="12"/><line x1="9" y1="15" x2="15" y2="15"/></svg>"#))
                                }
                                div class="action-card-title" { "New file" }
                                div class="action-card-desc" { "Create a markdown document" }
                            }
                            button class="action-card" type="button" onclick="showNewFolderModal()" {
                                div class="action-card-icon" {
                                    (PreEscaped(r#"<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/><line x1="12" y1="11" x2="12" y2="17"/><line x1="9" y1="14" x2="15" y2="14"/></svg>"#))
                                }
                                div class="action-card-title" { "New folder" }
                                div class="action-card-desc" { "Organise into directories" }
                            }
                        }
                        p class="dashboard-tip" {
                            "Right-click any file or folder in the sidebar for more options."
                        }
                    }
                }
            }
        },
    )
}

// ── Editor page ────────────────────────────────────────────────────────────────

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
                        button id="sidebar-toggle" class="hamburger" type="button" aria-label="Toggle sidebar" {
                            (PreEscaped(HAMBURGER_SVG))
                        }
                        span class="editor-path" { (rel_path) }
                        span id="save-status" class="save-status" {}
                        button
                            id="toolbar-rename"
                            class="toolbar-btn"
                            type="button"
                            data-path=(rel_path)
                        { "Rename" }
                        button
                            id="toolbar-delete"
                            class="toolbar-btn toolbar-btn-danger"
                            type="button"
                            data-path=(rel_path)
                            title="Delete file"
                        {
                            (PreEscaped(r#"<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/><path d="M10 11v6"/><path d="M14 11v6"/><path d="M9 6V4h6v2"/></svg>"#))
                            " Delete"
                        }
                    }
                    div class="pane-tabs" {
                        button class="pane-tab active" data-pane="editor" type="button" { "Editor" }
                        button class="pane-tab" data-pane="preview" type="button" { "Preview" }
                    }
                    div class="editor-panes" data-active="editor" {
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
                }
            }
            (codemirror_init())
        },
    )
}

// ── Sidebar ────────────────────────────────────────────────────────────────────

fn sidebar(tree: &[FileNode], active: Option<&str>) -> Markup {
    html! {
        aside class="sidebar" {
            div class="sidebar-header" {
                a href="/edit" class="brand" { "md" span { "·" } "server" }
                form method="post" action="/edit/logout" style="display:inline" {
                    button type="submit" class="btn-signout" { "Sign out" }
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
                    FileNode::Dir { name, rel, children } => {
                        li class="tree-dir"
                           data-tree-type="dir"
                           data-tree-path=(rel)
                        {
                            details open {
                                summary {
                                    (PreEscaped(r#"<svg class="dir-chevron" width="8" height="8" viewBox="0 0 8 8" fill="currentColor"><path d="M2 1l4 3-4 3V1z"/></svg>"#))
                                    (name)
                                }
                                (render_tree(children, active))
                            }
                        }
                    }
                    FileNode::File { name, rel } => {
                        @let is_active = active == Some(rel.as_str());
                        @let href = format!("/edit/open?path={}", urlencoded(rel));
                        li class=(if is_active { "tree-file active" } else { "tree-file" })
                           data-tree-type="file"
                           data-tree-path=(rel)
                        {
                            a href=(href) { (name) }
                        }
                    }
                }
            }
        }
    }
}

// ── CDN tags ───────────────────────────────────────────────────────────────────

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
      'Ctrl-S': function(cm) { cm.save(); htmx.trigger(ta, 'save-shortcut'); },
      'Cmd-S':  function(cm) { cm.save(); htmx.trigger(ta, 'save-shortcut'); }
    }
  });

  cm.on('change', function () {
    cm.save();
    htmx.trigger(ta, 'editor-change');
  });

  window.getEditorContent = function () { return cm.getValue(); };
})();
"#)) }
    }
}

// ── Shared assets ──────────────────────────────────────────────────────────────

const HAMBURGER_SVG: &str = r#"<svg width="18" height="18" viewBox="0 0 18 18" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round"><line x1="2" y1="4.5" x2="16" y2="4.5"/><line x1="2" y1="9" x2="16" y2="9"/><line x1="2" y1="13.5" x2="16" y2="13.5"/></svg>"#;

// ── CSS ────────────────────────────────────────────────────────────────────────

const BASE_CSS: &str = r#"
*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

:root {
  --bg:           #0d0f14;
  --surface:      #141720;
  --surface-2:    #1b2030;
  --surface-3:    #222840;
  --border:       #242a3d;
  --border-hi:    #2e3652;
  --text:         #dde1ed;
  --muted:        #68718f;
  --muted-2:      #404762;
  --accent:       #c9a84c;
  --accent-hi:    #ddbf6a;
  --accent-dim:   rgba(201,168,76,.12);
  --danger:       #e05555;
  --success:      #4caf82;
  --sidebar-w:    260px;
  --toolbar-h:    50px;
  --z-ctx:        1000;
  --z-modal:       900;
  --z-backdrop:    800;
  --z-sidebar:     700;
}

body {
  font-family: 'Lora', Georgia, serif;
  background: var(--bg);
  color: var(--text);
  height: 100vh;
  overflow: hidden;
  -webkit-font-smoothing: antialiased;
}

a { color: var(--accent); text-decoration: none; }
a:hover { color: var(--accent-hi); }

/* ── Login ── */
.login-wrap {
  max-width: 380px;
  margin: 12vh auto 0;
  padding: 2.5rem;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 14px;
}
.login-logo {
  font-family: 'Syne', sans-serif;
  font-size: 1.5rem;
  font-weight: 800;
  letter-spacing: -0.03em;
  color: var(--text);
  margin-bottom: 0.375rem;
}
.login-logo span { color: var(--accent); }
.login-sub {
  color: var(--muted);
  font-family: 'Syne', sans-serif;
  font-size: 0.8125rem;
  margin-bottom: 2rem;
}
.login-form { display: flex; flex-direction: column; gap: 0; }
.form-group { display: flex; flex-direction: column; gap: 0.375rem; margin-bottom: 1rem; }
.login-form label {
  font-family: 'Syne', sans-serif;
  font-size: 0.7rem;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.09em;
  color: var(--muted);
}
.login-form input {
  padding: 0.625rem 0.875rem;
  background: var(--surface-2);
  border: 1px solid var(--border);
  border-radius: 8px;
  font-size: 0.9375rem;
  font-family: 'Lora', serif;
  color: var(--text);
  outline: none;
  transition: border-color 0.15s, box-shadow 0.15s;
}
.login-form input:focus {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px rgba(201,168,76,.12);
}
.login-form button {
  margin-top: 0.5rem;
  padding: 0.7rem;
  background: var(--accent);
  color: #0d0f14;
  border: none;
  border-radius: 8px;
  font-family: 'Syne', sans-serif;
  font-size: 0.9375rem;
  font-weight: 700;
  cursor: pointer;
  transition: background 0.15s;
}
.login-form button:hover { background: var(--accent-hi); }
.error {
  color: var(--danger);
  font-size: 0.8125rem;
  background: rgba(224,85,85,.08);
  border: 1px solid rgba(224,85,85,.25);
  padding: 0.5rem 0.75rem;
  border-radius: 6px;
  margin-bottom: 1rem;
}

/* ── Layout ── */
.layout { display: flex; height: 100vh; overflow: hidden; }

/* ── Sidebar backdrop ── */
.sidebar-backdrop {
  display: none;
  position: fixed;
  inset: 0;
  background: rgba(0,0,0,.55);
  backdrop-filter: blur(2px);
  z-index: var(--z-backdrop);
}
body.sidebar-open .sidebar-backdrop { display: block; }

/* ── Sidebar ── */
.sidebar {
  width: var(--sidebar-w);
  flex-shrink: 0;
  background: var(--surface);
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  z-index: var(--z-sidebar);
  transition: transform 0.22s cubic-bezier(0.4,0,0.2,1);
}
.sidebar-header {
  height: var(--toolbar-h);
  padding: 0 1rem;
  border-bottom: 1px solid var(--border);
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-shrink: 0;
}
.brand {
  font-family: 'Syne', sans-serif;
  font-weight: 800;
  font-size: 1rem;
  letter-spacing: -0.03em;
  color: var(--text);
}
.brand span { color: var(--accent); }
.btn-signout {
  background: none;
  border: none;
  color: var(--muted);
  font-family: 'Syne', sans-serif;
  font-size: 0.75rem;
  cursor: pointer;
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
  transition: color 0.15s, background 0.15s;
}
.btn-signout:hover { color: var(--text); background: var(--surface-2); }

.file-tree {
  flex: 1;
  overflow-y: auto;
  padding: 0.5rem 0;
  font-family: 'Syne', sans-serif;
  font-size: 0.8125rem;
}
.file-tree::-webkit-scrollbar { width: 4px; }
.file-tree::-webkit-scrollbar-thumb { background: var(--border-hi); border-radius: 2px; }
.file-tree ul { list-style: none; padding-left: 1rem; }
.file-tree > ul { padding-left: 0.375rem; }

.tree-file a {
  display: block;
  padding: 0.3rem 0.75rem;
  border-radius: 5px;
  color: var(--muted);
  transition: color 0.1s, background 0.1s;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.tree-file a:hover { color: var(--text); background: var(--surface-2); }
.tree-file.active a {
  background: var(--accent-dim);
  color: var(--accent);
}

.tree-dir details > summary {
  display: flex;
  align-items: center;
  gap: 0.375rem;
  padding: 0.3rem 0.75rem;
  cursor: pointer;
  font-weight: 600;
  color: var(--text);
  border-radius: 5px;
  transition: background 0.1s;
  user-select: none;
  list-style: none;
}
.tree-dir details > summary::-webkit-details-marker { display: none; }
.tree-dir details > summary:hover { background: var(--surface-2); }
.dir-chevron {
  color: var(--muted-2);
  flex-shrink: 0;
  transition: transform 0.15s;
}
.tree-dir details[open] > summary .dir-chevron { transform: rotate(90deg); }

.sidebar-footer {
  padding: 0.75rem 1rem;
  border-top: 1px solid var(--border);
  font-family: 'Syne', sans-serif;
  font-size: 0.75rem;
}
.sidebar-footer a { color: var(--muted); }
.sidebar-footer a:hover { color: var(--accent); }

/* ── Main content ── */
.main-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  min-width: 0;
}

/* ── Hamburger ── */
.hamburger {
  display: none;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  background: none;
  border: 1px solid var(--border);
  border-radius: 7px;
  color: var(--muted);
  cursor: pointer;
  flex-shrink: 0;
  transition: color 0.15s, border-color 0.15s, background 0.15s;
}
.hamburger:hover { color: var(--text); border-color: var(--border-hi); background: var(--surface-2); }

/* ── Page topbar (dashboard, mobile) ── */
.page-topbar {
  display: none;
  height: var(--toolbar-h);
  padding: 0 1rem;
  border-bottom: 1px solid var(--border);
  background: var(--surface);
  align-items: center;
  gap: 0.875rem;
  flex-shrink: 0;
}
.topbar-title {
  font-family: 'Syne', sans-serif;
  font-weight: 800;
  font-size: 0.9375rem;
  color: var(--text);
  letter-spacing: -0.02em;
}

/* ── Dashboard ── */
.dashboard {
  padding: 2.5rem;
  overflow-y: auto;
  flex: 1;
}
.dashboard-header { margin-bottom: 2rem; }
.dashboard-title {
  font-family: 'Syne', sans-serif;
  font-size: 1.625rem;
  font-weight: 800;
  letter-spacing: -0.03em;
  margin-bottom: 0.375rem;
}
.dashboard-subtitle {
  color: var(--muted);
  font-size: 0.9375rem;
}
.action-cards {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 0.875rem;
  max-width: 500px;
  margin-bottom: 2rem;
}
.action-card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 1.375rem 1.5rem;
  text-align: left;
  cursor: pointer;
  font-family: inherit;
  transition: border-color 0.15s, background 0.15s, transform 0.12s;
}
.action-card:hover {
  border-color: var(--border-hi);
  background: var(--surface-2);
  transform: translateY(-1px);
}
.action-card-icon {
  color: var(--accent);
  margin-bottom: 0.75rem;
  display: flex;
}
.action-card-title {
  font-family: 'Syne', sans-serif;
  font-weight: 700;
  font-size: 0.9375rem;
  color: var(--text);
  margin-bottom: 0.25rem;
}
.action-card-desc {
  font-size: 0.8125rem;
  color: var(--muted);
  font-family: 'Syne', sans-serif;
}
.dashboard-tip {
  font-size: 0.8125rem;
  color: var(--muted-2);
  font-family: 'Syne', sans-serif;
}

/* ── Modal ── */
.modal-overlay {
  display: none;
  position: fixed;
  inset: 0;
  background: rgba(0,0,0,.65);
  backdrop-filter: blur(4px);
  z-index: var(--z-modal);
  align-items: center;
  justify-content: center;
}
.modal-overlay.active { display: flex; }
.modal {
  background: var(--surface);
  border: 1px solid var(--border-hi);
  border-radius: 12px;
  padding: 1.75rem;
  width: 420px;
  max-width: calc(100vw - 2rem);
  box-shadow: 0 24px 60px rgba(0,0,0,.6);
  animation: modal-in 0.17s cubic-bezier(0.34,1.56,0.64,1);
}
@keyframes modal-in {
  from { opacity: 0; transform: scale(0.92) translateY(-6px); }
  to   { opacity: 1; transform: scale(1)    translateY(0); }
}
.modal-title {
  font-family: 'Syne', sans-serif;
  font-weight: 700;
  font-size: 1rem;
  color: var(--text);
  margin-bottom: 1.125rem;
}
.modal-input {
  width: 100%;
  padding: 0.625rem 0.875rem;
  background: var(--surface-2);
  border: 1px solid var(--border);
  border-radius: 8px;
  font-size: 0.875rem;
  font-family: 'JetBrains Mono', monospace;
  color: var(--text);
  outline: none;
  transition: border-color 0.15s;
  margin-bottom: 1.25rem;
}
.modal-input:focus { border-color: var(--accent); }
.modal-actions { display: flex; gap: 0.625rem; justify-content: flex-end; }

.btn-primary {
  padding: 0.5rem 1.125rem;
  background: var(--accent);
  color: #0d0f14;
  border: none;
  border-radius: 7px;
  font-family: 'Syne', sans-serif;
  font-weight: 700;
  font-size: 0.875rem;
  cursor: pointer;
  transition: background 0.15s;
}
.btn-primary:hover { background: var(--accent-hi); }
.btn-secondary {
  padding: 0.5rem 1rem;
  background: transparent;
  color: var(--muted);
  border: 1px solid var(--border);
  border-radius: 7px;
  font-family: 'Syne', sans-serif;
  font-size: 0.875rem;
  cursor: pointer;
  transition: color 0.15s, border-color 0.15s;
}
.btn-secondary:hover { color: var(--text); border-color: var(--border-hi); }

/* ── Editor toolbar ── */
.editor-toolbar {
  height: var(--toolbar-h);
  padding: 0 1rem;
  border-bottom: 1px solid var(--border);
  background: var(--surface);
  display: flex;
  align-items: center;
  gap: 0.625rem;
  flex-shrink: 0;
}
.editor-path {
  flex: 1;
  font-family: 'JetBrains Mono', monospace;
  font-size: 0.8rem;
  color: var(--muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  min-width: 0;
}
.save-status { font-family: 'Syne', sans-serif; font-size: 0.75rem; white-space: nowrap; }
.save-ok { color: var(--success); font-family: 'Syne', sans-serif; font-size: 0.75rem; }

.toolbar-btn {
  display: flex;
  align-items: center;
  gap: 0.3rem;
  padding: 0.375rem 0.75rem;
  background: transparent;
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--muted);
  font-family: 'Syne', sans-serif;
  font-size: 0.75rem;
  font-weight: 600;
  cursor: pointer;
  white-space: nowrap;
  transition: color 0.15s, border-color 0.15s, background 0.15s;
}
.toolbar-btn:hover { color: var(--text); border-color: var(--border-hi); background: var(--surface-2); }
.toolbar-btn-danger { color: var(--muted); }
.toolbar-btn-danger:hover {
  color: var(--danger);
  border-color: rgba(224,85,85,.35);
  background: rgba(224,85,85,.08);
}

/* ── Pane tabs (mobile only) ── */
.pane-tabs {
  display: none;
  height: 38px;
  background: var(--surface);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}
.pane-tab {
  flex: 1;
  background: none;
  border: none;
  border-bottom: 2px solid transparent;
  color: var(--muted);
  font-family: 'Syne', sans-serif;
  font-size: 0.8125rem;
  font-weight: 600;
  cursor: pointer;
  transition: color 0.15s, border-color 0.15s;
}
.pane-tab.active { color: var(--accent); border-bottom-color: var(--accent); }

/* ── Editor panes ── */
.editor-panes { display: flex; flex: 1; overflow: hidden; }
.pane { flex: 1; overflow: auto; min-width: 0; }
.pane-editor {
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
}
#editor-form {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.pane-editor .CodeMirror {
  flex: 1;
  height: 100%;
  font-family: 'JetBrains Mono', monospace;
  font-size: 0.875rem;
  line-height: 1.7;
  background: var(--bg) !important;
  color: var(--text) !important;
}
.CodeMirror-gutters {
  background: var(--surface) !important;
  border-right: 1px solid var(--border) !important;
}
.CodeMirror-linenumber { color: var(--muted-2) !important; }
.CodeMirror-cursor { border-left-color: var(--accent) !important; }
.CodeMirror-selected { background: rgba(201,168,76,.13) !important; }
.cm-header { color: var(--accent-hi) !important; font-weight: 700; }
.cm-strong { font-weight: 700; }
.cm-em { font-style: italic; }
.cm-link, .cm-url { color: var(--accent) !important; }
.cm-comment { color: var(--muted) !important; font-style: italic; }

/* ── Preview pane ── */
.pane-preview {
  padding: 2rem 2.75rem;
  font-family: 'Lora', Georgia, serif;
  font-size: 1rem;
  line-height: 1.8;
  color: var(--text);
  background: var(--bg);
  overflow-y: auto;
}
.pane-preview::-webkit-scrollbar { width: 4px; }
.pane-preview::-webkit-scrollbar-thumb { background: var(--border-hi); border-radius: 2px; }
.pane-preview h1,
.pane-preview h2,
.pane-preview h3,
.pane-preview h4,
.pane-preview h5,
.pane-preview h6 {
  font-family: 'Syne', sans-serif;
  font-weight: 700;
  line-height: 1.3;
  letter-spacing: -0.025em;
  margin: 1.5em 0 0.5em;
  color: var(--text);
}
.pane-preview h1 { font-size: 1.875rem; font-weight: 800; }
.pane-preview h2 { font-size: 1.375rem; }
.pane-preview h3 { font-size: 1.125rem; }
.pane-preview p { margin: 0.875em 0; }
.pane-preview pre {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 1.125em 1.25em;
  overflow-x: auto;
  margin: 1em 0;
}
.pane-preview code {
  font-family: 'JetBrains Mono', monospace;
  font-size: 0.82em;
  background: var(--surface);
  border: 1px solid var(--border);
  padding: 0.15em 0.35em;
  border-radius: 4px;
}
.pane-preview pre code { background: none; border: none; padding: 0; font-size: 0.875em; }
.pane-preview blockquote {
  border-left: 3px solid var(--accent);
  padding: 0.25em 1em;
  color: var(--muted);
  margin: 1em 0;
  font-style: italic;
}
.pane-preview a { color: var(--accent); }
.pane-preview a:hover { color: var(--accent-hi); }
.pane-preview ul, .pane-preview ol { padding-left: 1.75em; margin: 0.75em 0; }
.pane-preview li { margin: 0.25em 0; }
.pane-preview hr { border: none; border-top: 1px solid var(--border); margin: 2em 0; }
.pane-preview table { border-collapse: collapse; width: 100%; margin: 1em 0; font-size: 0.9375em; }
.pane-preview th, .pane-preview td {
  padding: 0.5em 0.875em;
  border: 1px solid var(--border);
  text-align: left;
}
.pane-preview th { background: var(--surface); font-family: 'Syne', sans-serif; font-weight: 700; }

/* ── Context menu ── */
.context-menu {
  position: fixed;
  z-index: var(--z-ctx);
  background: var(--surface-2);
  border: 1px solid var(--border-hi);
  border-radius: 9px;
  padding: 0.375rem 0;
  min-width: 178px;
  box-shadow: 0 16px 40px rgba(0,0,0,.55), 0 2px 8px rgba(0,0,0,.3);
  display: none;
  animation: ctx-in 0.1s ease-out;
}
.context-menu.active { display: block; }
@keyframes ctx-in {
  from { opacity: 0; transform: scale(0.94) translateY(-4px); }
  to   { opacity: 1; transform: scale(1)    translateY(0); }
}
.ctx-item {
  display: flex;
  align-items: center;
  gap: 0.625rem;
  padding: 0.5rem 1rem;
  font-family: 'Syne', sans-serif;
  font-size: 0.8125rem;
  color: var(--text);
  cursor: pointer;
  transition: background 0.1s;
}
.ctx-item:hover { background: var(--surface-3); }
.ctx-item.danger { color: var(--danger); }
.ctx-item.danger:hover { background: rgba(224,85,85,.1); }
.ctx-separator { height: 1px; background: var(--border); margin: 0.375rem 0; }
.ctx-icon { width: 14px; text-align: center; opacity: 0.65; }

/* ── Mobile responsive ── */
@media (max-width: 768px) {
  :root { --sidebar-w: 280px; }

  .sidebar {
    position: fixed;
    inset-block: 0;
    left: 0;
    transform: translateX(calc(-1 * var(--sidebar-w) - 2px));
  }
  body.sidebar-open .sidebar { transform: translateX(0); }

  .hamburger { display: flex; }
  .page-topbar { display: flex; }
  .pane-tabs { display: flex; }

  .editor-panes[data-active="preview"] .pane-editor { display: none; }
  .editor-panes[data-active="editor"]  .pane-preview { display: none; }
  .pane-editor { border-right: none; }

  .editor-path { display: none; }
  .action-cards { grid-template-columns: 1fr; max-width: 100%; }
  .dashboard { padding: 1.25rem; }

  .pane-preview { padding: 1.25rem 1.5rem; }
}
"#;

// ── JavaScript ─────────────────────────────────────────────────────────────────

const UI_JS: &str = r#"
(function () {
  'use strict';

  // ── Modal ──────────────────────────────────────────────────────────────────
  var modalOverlay = document.getElementById('modal-overlay');
  var modalTitle   = document.getElementById('modal-title');
  var modalInput   = document.getElementById('modal-input');
  var modalConfirm = document.getElementById('modal-confirm');
  var modalCancel  = document.getElementById('modal-cancel');

  function showModal(title, defaultVal, onConfirm) {
    if (!modalOverlay) return;
    modalTitle.textContent = title;
    modalInput.value = defaultVal;
    modalOverlay.classList.add('active');
    setTimeout(function () { modalInput.focus(); modalInput.select(); }, 40);

    function close() {
      modalOverlay.classList.remove('active');
      modalOverlay.onclick = null;
      modalConfirm.onclick = null;
      modalCancel.onclick  = null;
      modalInput.onkeydown = null;
    }
    function submit() {
      var val = modalInput.value.trim();
      if (val) { close(); onConfirm(val); }
    }
    modalConfirm.onclick = submit;
    modalCancel.onclick  = close;
    modalInput.onkeydown = function (e) {
      if (e.key === 'Enter')  submit();
      if (e.key === 'Escape') close();
    };
    modalOverlay.onclick = function (e) { if (e.target === modalOverlay) close(); };
  }

  // ── Hidden form helper ─────────────────────────────────────────────────────
  function submitForm(id, fields) {
    var form = document.getElementById(id);
    if (!form) return;
    Object.keys(fields).forEach(function (k) {
      form.querySelector('[name="' + k + '"]').value = fields[k];
    });
    form.submit();
  }

  // ── Dashboard actions (exposed as globals) ─────────────────────────────────
  window.showNewFileModal = function () {
    showModal('New file', 'untitled.md', function (val) {
      submitForm('hidden-new-file-form', { path: val });
    });
  };
  window.showNewFolderModal = function () {
    showModal('New folder', 'new-folder', function (val) {
      submitForm('hidden-new-dir-form', { path: val });
    });
  };

  // ── Context menu ───────────────────────────────────────────────────────────
  var ctxMenu   = document.getElementById('context-menu');
  var ctxTarget = null;

  function hideCtx() {
    if (ctxMenu) ctxMenu.classList.remove('active');
    ctxTarget = null;
  }

  if (ctxMenu) {
    document.addEventListener('contextmenu', function (e) {
      var item = e.target.closest('[data-tree-type]');
      if (!item) return;
      e.preventDefault();
      ctxTarget = item;

      var type = item.dataset.treeType;
      ctxMenu.querySelectorAll('[data-ctx-for]').forEach(function (el) {
        var forTypes = el.dataset.ctxFor.split(',');
        el.style.display = forTypes.indexOf(type) >= 0 ? '' : 'none';
      });

      ctxMenu.classList.add('active');
      ctxMenu.style.left = e.clientX + 'px';
      ctxMenu.style.top  = e.clientY + 'px';

      // Clamp to viewport
      requestAnimationFrame(function () {
        var r = ctxMenu.getBoundingClientRect();
        if (r.right  > window.innerWidth)  ctxMenu.style.left = (e.clientX - r.width) + 'px';
        if (r.bottom > window.innerHeight) ctxMenu.style.top  = (e.clientY - r.height) + 'px';
      });
    });

    document.addEventListener('click',   hideCtx);
    document.addEventListener('keydown', function (e) { if (e.key === 'Escape') hideCtx(); });
    ctxMenu.addEventListener('click', function (e) { e.stopPropagation(); });

    ctxMenu.addEventListener('click', function (e) {
      var item = e.target.closest('.ctx-item');
      if (!item || !ctxTarget) return;
      var action = item.dataset.action;
      var path   = ctxTarget.dataset.treePath;
      hideCtx();

      if (action === 'open') {
        window.location.href = '/edit/open?path=' + encodeURIComponent(path);
      } else if (action === 'new-file') {
        showModal('New file in ' + path, path + '/untitled.md', function (val) {
          submitForm('hidden-new-file-form', { path: val });
        });
      } else if (action === 'new-folder') {
        showModal('New folder in ' + path, path + '/new-folder', function (val) {
          submitForm('hidden-new-dir-form', { path: val });
        });
      } else if (action === 'rename') {
        showModal('Rename / move', path, function (val) {
          submitForm('hidden-rename-form', { old_path: path, new_path: val });
        });
      } else if (action === 'delete') {
        if (confirm('Delete "' + path + '"? This cannot be undone.')) {
          fetch('/edit/delete?path=' + encodeURIComponent(path), { method: 'DELETE' })
            .then(function () { window.location.href = '/edit'; });
        }
      }
    });
  }

  // ── Mobile sidebar ─────────────────────────────────────────────────────────
  var toggle   = document.getElementById('sidebar-toggle');
  var backdrop = document.querySelector('.sidebar-backdrop');
  if (toggle) {
    toggle.addEventListener('click', function () {
      document.body.classList.toggle('sidebar-open');
    });
  }
  if (backdrop) {
    backdrop.addEventListener('click', function () {
      document.body.classList.remove('sidebar-open');
    });
  }
  document.addEventListener('keydown', function (e) {
    if (e.key === 'Escape') document.body.classList.remove('sidebar-open');
  });

  // ── Pane tabs ──────────────────────────────────────────────────────────────
  document.querySelectorAll('.pane-tab').forEach(function (tab) {
    tab.addEventListener('click', function () {
      document.querySelectorAll('.pane-tab').forEach(function (t) {
        t.classList.remove('active');
      });
      tab.classList.add('active');
      var panes = document.querySelector('.editor-panes');
      if (panes) panes.dataset.active = tab.dataset.pane;
    });
  });

  // ── Editor toolbar buttons ─────────────────────────────────────────────────
  var renameBtn = document.getElementById('toolbar-rename');
  if (renameBtn) {
    renameBtn.addEventListener('click', function () {
      var path = renameBtn.dataset.path;
      showModal('Rename / move', path, function (val) {
        submitForm('hidden-rename-form', { old_path: path, new_path: val });
      });
    });
  }

  var deleteBtn = document.getElementById('toolbar-delete');
  if (deleteBtn) {
    deleteBtn.addEventListener('click', function () {
      var path = deleteBtn.dataset.path;
      if (confirm('Delete "' + path + '"? This cannot be undone.')) {
        fetch('/edit/delete?path=' + encodeURIComponent(path), { method: 'DELETE' })
          .then(function () { window.location.href = '/edit'; });
      }
    });
  }

})();
"#;
