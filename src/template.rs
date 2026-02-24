use maud::{html, Markup, PreEscaped, DOCTYPE};

use crate::front_matter::FrontMatter;

pub struct DirEntry {
    pub display_name: String,
    pub url: String,
    pub is_dir: bool,
    pub title: Option<String>,
    pub date: Option<String>,
    pub summary: Option<String>,
    pub author: Option<String>,
}

/// Full HTML page wrapping rendered markdown content.
pub fn page(fm: &FrontMatter, content_html: &str, css_path: Option<&str>) -> Markup {
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
