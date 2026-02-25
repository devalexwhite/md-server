use axum::{
    extract::{ConnectInfo, Request, State},
    middleware::Next,
    response::Response,
};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::net::{IpAddr, SocketAddr};

use crate::state::AppState;

/// Returns true if `ip` is a loopback or private address, indicating the
/// request came through a trusted local reverse proxy.
fn is_trusted_proxy(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_loopback(),
    }
}

pub async fn log_request(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();

    // Skip health check and all editor routes.
    if path == "/healthz" || path == "/edit" || path.starts_with("/edit/") {
        return next.run(req).await;
    }

    // Collect what we need before consuming `req`.
    let referer = req
        .headers()
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let ua_str = req
        .headers()
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let conn_ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip());

    // Only trust X-Forwarded-For when the direct connection comes from a
    // trusted local address (loopback/private), indicating a reverse proxy.
    // Otherwise use the socket address directly to prevent IP spoofing.
    let ip = if conn_ip.map_or(false, is_trusted_proxy) {
        req.headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').next())
            .map(|s| s.trim().to_string())
            .or_else(|| conn_ip.map(|ip| ip.to_string()))
    } else {
        conn_ip.map(|ip| ip.to_string())
    };

    // Hash IP + current UTC date so individual IPs are not stored in plain text
    // and the hash rotates daily.
    let ip_hash = ip.as_deref().map(|ip_str| {
        let date = Utc::now().format("%Y-%m-%d").to_string();
        let mut h = Sha256::new();
        h.update(ip_str.as_bytes());
        h.update(date.as_bytes());
        format!("{:x}", h.finalize())
    });

    let (browser, os) = parse_ua(ua_str.as_deref());

    let db = state.db.clone();

    // Fire-and-forget: log asynchronously so we never slow down the response.
    tokio::spawn(async move {
        if let Err(e) =
            crate::db::insert_request(&db, &path, referer.as_deref(), ip_hash.as_deref(), browser.as_deref(), os.as_deref()).await
        {
            tracing::warn!("Failed to log request: {}", e);
        }
    });

    next.run(req).await
}

fn parse_ua(ua: Option<&str>) -> (Option<String>, Option<String>) {
    let ua = match ua {
        Some(s) if !s.is_empty() => s,
        _ => return (None, None),
    };

    let parser = woothee::parser::Parser::new();
    match parser.parse(ua) {
        Some(r) => {
            let browser = (r.name != "UNKNOWN").then(|| r.name.to_string());
            let os = (r.os != "UNKNOWN").then(|| r.os.to_string());
            (browser, os)
        }
        None => (None, None),
    }
}
