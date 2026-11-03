//! Neko AI server: auth, provider keys, chat storage and streaming proxy to LLM providers.

pub mod api;
pub mod auth;
pub mod config;
pub mod crypto;
pub mod db;
pub mod devices;
pub mod error;
pub mod llm;

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
                setup_code: Mutex::new(setup_code),
                login_failures: Mutex::new(HashMap::new()),
            }),
        }
    }
}

pub fn router(state: AppState) -> Router {
    let web_dir = state.config.web_dir.clone();
    let static_files = ServeDir::new(&web_dir).fallback(ServeFile::new(web_dir.join("index.html")));

    Router::new()
        .nest("/api", api::routes())
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
