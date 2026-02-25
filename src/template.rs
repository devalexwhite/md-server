use maud::{DOCTYPE, Markup, PreEscaped, html};

use crate::front_matter::FrontMatter;

pub struct Breadcrumb {
    pub label: String,
    /// `None` for the current (last) crumb — rendered without a link.
    pub url: Option<String>,
}

/// Build breadcrumbs from a URL path.
///
/// Always starts with a "Home" entry linking to `/`. The last entry has
/// `url: None` (the current page). Returns only `[Home (current)]` for the
/// root, in which case the caller should skip rendering the nav.
pub fn build_breadcrumbs(url_path: &str) -> Vec<Breadcrumb> {
    let path = url_path.trim_end_matches('/');

    if path.is_empty() {
        return vec![Breadcrumb {
            label: "Home".to_string(),
            url: None,
        }];
    }

    let segments: Vec<&str> = path
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    let mut crumbs = vec![Breadcrumb {
        label: "Home".to_string(),
        url: Some("/".to_string()),
    }];

    for (i, &segment) in segments.iter().enumerate() {
        let is_last = i == segments.len() - 1;
        let label = if is_last {
            segment.strip_suffix(".md").unwrap_or(segment).to_string()
        } else {
            segment.to_string()
        };

        if is_last {
            crumbs.push(Breadcrumb { label, url: None });
        } else {
            crumbs.push(Breadcrumb {
                label,
                url: Some(format!("/{}/", segments[..=i].join("/"))),
            });
        }
    }

    crumbs
}

pub struct DirEntry {
    pub display_name: String,
    pub url: String,
    pub is_dir: bool,
    pub title: Option<String>,
    pub date: Option<String>,
    pub summary: Option<String>,
    pub author: Option<String>,
    pub content: Option<String>,
}

/// Full HTML page wrapping rendered markdown content.
pub fn page(
    fm: &FrontMatter,
    content_html: &str,
    css_path: Option<&str>,
    meta_image: Option<&str>,
    breadcrumbs: &[Breadcrumb],
) -> Markup {
    let title = fm.title.as_deref().unwrap_or("");
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) }
                @if !title.is_empty() {
                    meta property="og:title" content=(title);
                }
                @if let Some(s) = &fm.summary {
                    meta name="description" content=(s);
                    meta property="og:description" content=(s);
                }
                @if let Some(img) = meta_image {
                    meta property="og:image" content=(img);
                    meta name="twitter:card" content="summary_large_image";
                    meta name="twitter:image" content=(img);
                }
                @if let Some(a) = &fm.author {
                    meta name="author" content=(a);
                }
                @if let Some(d) = &fm.date {
                    meta property="article:published_time" content=(d);
                }
                @if let Some(css) = css_path {
                    link rel="stylesheet" href=(css);
                }
            }
            body {
                @if breadcrumbs.len() > 1 {
                    nav aria-label="breadcrumb" {
                        ol {
                            @for crumb in breadcrumbs {
                                li {
                                    @if let Some(url) = &crumb.url {
                                        a href=(url) { (crumb.label) }
                                    } @else {
                                        span aria-current="page" { (crumb.label) }
                                    }
                                }
                            }
                        }
                    }
                }
                main {
                    (PreEscaped(content_html))
                }
            }
        }
    }
}

/// Directory listing page.
pub fn directory_index(dir_url: &str, entries: &[DirEntry], css_path: Option<&str>) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Index of " (dir_url) }
                @if let Some(css) = css_path {
                    link rel="stylesheet" href=(css);
                }
            }
            body {
                main {
                    h1 { "Index of " (dir_url) }
                    @if entries.is_empty() {
                        p { em { "Empty directory." } }
                    } @else {
                        ul {
                            @for e in entries {
                                li {
                                    a href=(e.url) {
                                        @if e.is_dir {
                                            (e.display_name) "/"
                                        } @else {
                                            (e.title.as_deref().unwrap_or(&e.display_name))
                                        }
                                    }
                                    @if let Some(d) = &e.date {
                                        " — " (d)
                                    }
                                    @if let Some(a) = &e.author {
                                        " by " (a)
                                    }
                                    @if let Some(s) = &e.summary {
                                        p { (s) }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
