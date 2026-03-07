use serde::Serialize;
use std::collections::HashMap;

// ── Incoming request types ────────────────────────────────────────────────────

/// Unified request type produced by both form-encoded and JSON parsing paths.
#[derive(Debug)]
pub enum MicropubRequest {
    Create(CreateEntry),
    Update(UpdateRequest),
    Delete { url: String },
    Undelete { url: String },
}

/// Parsed h=entry create payload, normalised from either wire format.
#[derive(Debug, Default)]
pub struct CreateEntry {
    /// Article title (`name` in Microformats2). Absent → timestamp-based slug.
    pub name: Option<String>,
    /// Post body content.
    pub content: String,
    /// Tags/categories (`category` in Microformats2).
    pub tags: Vec<String>,
    /// Optional client-supplied slug override (`mp-slug`).
    pub slug: Option<String>,
    /// ISO 8601 publication date. Defaults to today if absent.
    pub published: Option<String>,
    /// Micropub `post-status`: `"draft"` or `"published"`. Defaults to `"published"`.
    pub post_status: Option<String>,
}

/// JSON update request body (POST with `"action": "update"`).
#[derive(Debug)]
pub struct UpdateRequest {
    pub url: String,
    /// Properties to replace wholesale. Map of property name → new values.
    pub replace: HashMap<String, Vec<serde_json::Value>>,
    /// Properties to add values to. Map of property name → values to add.
    pub add: HashMap<String, Vec<serde_json::Value>>,
    /// Property names to remove entirely.
    pub delete: Vec<String>,
}

// ── Error response ────────────────────────────────────────────────────────────

/// Serialises to the W3C Micropub error JSON shape:
/// `{"error": "...", "error_description": "..."}`.
#[derive(Debug, Serialize)]
pub struct MicropubError {
    pub error: String,
    pub error_description: String,
}

impl MicropubError {
    pub fn new(error: impl Into<String>, description: impl Into<String>) -> Self {
        MicropubError {
            error: error.into(),
            error_description: description.into(),
        }
    }
}

// ── GET /micropub response types ──────────────────────────────────────────────

/// Response body for `GET /micropub?q=config`.
#[derive(Serialize)]
pub struct MicropubConfig {
    #[serde(rename = "media-endpoint")]
    pub media_endpoint: String,
    #[serde(rename = "syndicate-to")]
    pub syndicate_to: Vec<serde_json::Value>,
    #[serde(rename = "post-types")]
    pub post_types: Vec<PostTypeInfo>,
}

#[derive(Serialize)]
pub struct PostTypeInfo {
    #[serde(rename = "type")]
    pub post_type: String,
    pub name: String,
}

/// Response body for `GET /micropub?q=source&url=<url>`.
#[derive(Serialize)]
pub struct SourceResponse {
    #[serde(rename = "type")]
    pub post_type: Vec<String>,
    pub properties: SourceProperties,
}

#[derive(Serialize)]
pub struct SourceProperties {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub name: Vec<String>,
    pub content: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub category: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub published: Vec<String>,
    pub url: Vec<String>,
    #[serde(rename = "post-status")]
    pub post_status: String,
}
