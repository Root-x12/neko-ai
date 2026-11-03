//! Neko AI server: auth, provider keys, chat storage and streaming proxy to LLM providers.

pub mod admin;
pub mod api;
pub mod auth;
pub mod chat;
pub mod config;
pub mod crypto;
pub mod db;
pub mod devices;
pub mod error;
pub mod guard;
pub mod llm;
pub mod tools;
pub mod uploads;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use axum::{
    http::{header, HeaderValue},
    Router,
};
use tower_http::{
    compression::CompressionLayer,
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
};

pub use config::Config;

#[derive(Clone)]
pub struct AppState {
    pub inner: Arc<Inner>,
}

pub struct Inner {
    pub config: Config,
    pub db: db::Db,
    pub vault: crypto::Vault,
    pub http: reqwest::Client,
    pub mcp: tools::mcp::McpManager,
    /// Required for the first sign-up; `None` once an account exists.
    pub setup_code: Mutex<Option<String>>,
    /// username -> (failed attempts, time of last failure)
    pub login_failures: Mutex<HashMap<String, (u32, i64)>>,
}

impl std::ops::Deref for AppState {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.inner
    }
}

impl AppState {
    pub fn new(config: Config, db: db::Db, vault: crypto::Vault, setup_code: Option<String>) -> Self {
        Self {
            inner: Arc::new(Inner {
                config,
                db,
                vault,
                http: llm::http_client(),
                mcp: tools::mcp::McpManager::default(),
                setup_code: Mutex::new(setup_code),
                login_failures: Mutex::new(HashMap::new()),
            }),
        }
    }
}

const CANVAS_HTML: &str = include_str!("../assets/canvas.html");

/// The artifact preview runner. It is loaded in a sandboxed iframe, so it gets its own policy:
/// scripts only from this server (plus inline/eval for the user's code), no network access,
/// and it may only be framed by this app.
async fn canvas_page(headers: axum::http::HeaderMap) -> axum::response::Response {
    use axum::response::IntoResponse;
    // A sandboxed document has an opaque origin, so name this server explicitly besides 'self'.
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .filter(|h| h.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '[' | ']')))
        .unwrap_or("localhost");
    let me = format!("http://{host} https://{host}");
    let csp = format!(
        "default-src 'none'; script-src 'self' {me} 'unsafe-inline' 'unsafe-eval'; style-src 'self' {me} 'unsafe-inline'; \
         img-src 'self' {me} data: blob:; font-src 'self' {me} data:; connect-src 'none'; media-src data: blob:; \
         frame-ancestors 'self' {me}; base-uri 'none'; form-action 'none'"
    );
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()),
            (header::CONTENT_SECURITY_POLICY, csp),
            (header::X_FRAME_OPTIONS, "SAMEORIGIN".to_string()),
            (header::CACHE_CONTROL, "no-cache".to_string()),
        ],
        CANVAS_HTML,
    )
        .into_response()
}

pub fn router(state: AppState) -> Router {
    let web_dir = state.config.web_dir.clone();
    let static_files = ServeDir::new(&web_dir).fallback(ServeFile::new(web_dir.join("index.html")));

    Router::new()
        .nest("/api", api::routes())
        .route("/canvas.html", axum::routing::get(canvas_page))
        .fallback_service(static_files)
        .layer(CompressionLayer::new())
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer")))
        .layer(SetResponseHeaderLayer::if_not_present(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "default-src 'self'; script-src 'self' 'unsafe-inline' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; \
                 img-src 'self' data: blob:; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'",
            ),
        ))
        .with_state(state)
}
