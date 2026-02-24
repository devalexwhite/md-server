use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use maud::{html, DOCTYPE};

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("Not found")]
    NotFound,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, title, message) = match &self {
            AppError::NotFound => (
                StatusCode::NOT_FOUND,
                "404 Not Found",
                "The page you requested could not be found.".to_string(),
            ),
            AppError::Io(e) => {
                tracing::error!("IO error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "500 Internal Server Error",
                    "An internal server error occurred.".to_string(),
                )
            }
            AppError::Internal(msg) => {
                tracing::error!("Internal error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "500 Internal Server Error",
                    msg.clone(),
                )
            }
        };

        let body = html! {
            (DOCTYPE)
            html lang="en" {
                head {
                    meta charset="utf-8";
                    title { (title) }
                }
                body {
                    h1 { (title) }
                    p { (message) }
                }
            }
        };

        (status, Html(body.into_string())).into_response()
    }
}
