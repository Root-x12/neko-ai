use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use neko_shared::ApiError;

/// An error returned to the client as `{"error": "..."}`.
#[derive(Debug)]
pub struct AppError {
    pub status: StatusCode,
    pub message: String,
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self { status, message: message.into() }
    }
    pub fn bad_request(m: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, m)
    }
    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "Please sign in")
    }
    pub fn forbidden(m: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, m)
    }
    pub fn not_found(what: &str) -> Self {
        Self::new(StatusCode::NOT_FOUND, format!("{what} not found"))
    }
    pub fn bad_gateway(m: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, m)
    }
    pub fn internal(e: impl std::fmt::Display) -> Self {
        tracing::error!("internal error: {e}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong on the server")
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        Self::internal(e)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.status, Json(ApiError { error: self.message, findings: Vec::new() })).into_response()
    }
}
