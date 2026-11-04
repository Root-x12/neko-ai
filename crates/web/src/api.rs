//! Thin fetch wrappers around the server API.

use js_sys::{Reflect, Uint8Array};
use neko_shared::{sse::SseParser, ApiError as ErrorBody, StreamEvent};
use serde::{de::DeserializeOwned, Serialize};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{AbortSignal, Headers, ReadableStreamDefaultReader, Request, RequestCredentials, RequestInit, Response};

#[derive(Debug, Clone, PartialEq)]
pub struct ApiError {
    pub status: u16,
    pub message: String,
    /// Secrets/personal data that made the server hold a message back.
    pub findings: Vec<neko_shared::Finding>,
}

impl ApiError {
    fn network(e: JsValue) -> Self {
        let detail =
            e.dyn_ref::<js_sys::Error>().map(|e| String::from(e.message())).unwrap_or_else(|| format!("{e:?}"));
        Self { status: 0, message: format!("Can't reach the server ({detail})"), findings: Vec::new() }
    }

    pub fn is_abort(&self) -> bool {
        self.message.contains("AbortError") || self.message.contains("aborted")
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

enum Body {
    None,
    Json(String),
    Form(web_sys::FormData),
}

async fn fetch(method: &str, path: &str, body: Body, signal: Option<&AbortSignal>) -> ApiResult<Response> {
    let init = RequestInit::new();
    init.set_method(method);
    init.set_credentials(RequestCredentials::SameOrigin);
    let headers = Headers::new().map_err(ApiError::network)?;
    match body {
        Body::None => {}
        Body::Json(body) => {
            headers.set("content-type", "application/json").map_err(ApiError::network)?;
            init.set_body(&JsValue::from_str(&body));
        }
        // The browser sets the multipart boundary itself.
        Body::Form(form) => init.set_body(&form),
    }
    init.set_headers(&headers);
    if let Some(signal) = signal {
        init.set_signal(Some(signal));
    }
    let req = Request::new_with_str_and_init(&format!("/api{path}"), &init).map_err(ApiError::network)?;
    let window = web_sys::window().expect("window");
    let resp: Response =
        JsFuture::from(window.fetch_with_request(&req)).await.map_err(ApiError::network)?.unchecked_into();
    if resp.ok() {
        return Ok(resp);
    }
    let status = resp.status();
    let text = text(&resp).await.unwrap_or_default();
    let (message, findings) = serde_json::from_str::<ErrorBody>(&text)
        .map(|b| (b.error, b.findings))
        .unwrap_or_else(|_| (format!("Request failed ({status})"), Vec::new()));
    Err(ApiError { status, message, findings })
}

async fn text(resp: &Response) -> ApiResult<String> {
    let promise = resp.text().map_err(ApiError::network)?;
    Ok(JsFuture::from(promise).await.map_err(ApiError::network)?.as_string().unwrap_or_default())
}

async fn parse<T: DeserializeOwned>(resp: Response) -> ApiResult<T> {
    let body = text(&resp).await?;
    serde_json::from_str(&body).map_err(|e| ApiError {
        status: resp.status(),
        message: format!("Unexpected response: {e}"),
        findings: Vec::new(),
    })
}

pub async fn get<T: DeserializeOwned>(path: &str) -> ApiResult<T> {
    parse(fetch("GET", path, Body::None, None).await?).await
}

pub async fn send<B: Serialize, T: DeserializeOwned>(method: &str, path: &str, body: &B) -> ApiResult<T> {
    let json = serde_json::to_string(body).expect("serializable body");
    parse(fetch(method, path, Body::Json(json), None).await?).await
}

/// For endpoints that answer `204 No Content`.
pub async fn call(method: &str, path: &str) -> ApiResult<()> {
    fetch(method, path, Body::None, None).await.map(|_| ())
}

pub async fn call_with<B: Serialize>(method: &str, path: &str, body: &B) -> ApiResult<()> {
    let json = serde_json::to_string(body).expect("serializable body");
    fetch(method, path, Body::Json(json), None).await.map(|_| ())
}

/// POSTs `body` and calls `on_event` for each streamed event until the server closes the stream
/// or `signal` aborts it.
pub async fn stream<B: Serialize>(
    path: &str,
    body: &B,
    signal: &AbortSignal,
    mut on_event: impl FnMut(StreamEvent),
) -> ApiResult<()> {
    let json = serde_json::to_string(body).expect("serializable body");
    let resp = fetch("POST", path, Body::Json(json), Some(signal)).await?;
    let body =
        resp.body().ok_or_else(|| ApiError { status: 0, message: "Empty response".into(), findings: Vec::new() })?;
    let reader: ReadableStreamDefaultReader = body.get_reader().unchecked_into();
    let mut parser = SseParser::new();
    let done_key = JsValue::from_str("done");
    let value_key = JsValue::from_str("value");
    loop {
        let chunk = JsFuture::from(reader.read()).await.map_err(ApiError::network)?;
        if Reflect::get(&chunk, &done_key).ok().and_then(|v| v.as_bool()).unwrap_or(true) {
            break;
        }
        let Ok(value) = Reflect::get(&chunk, &value_key) else { continue };
        let bytes = Uint8Array::new(&value).to_vec();
        for ev in parser.push(&bytes) {
            if let Ok(ev) = serde_json::from_str::<StreamEvent>(&ev.data) {
                on_event(ev);
            }
        }
    }
    Ok(())
}

pub async fn upload(file: &web_sys::File) -> ApiResult<neko_shared::Attachment> {
    upload_to("/uploads", file).await
}

pub async fn upload_to<T: DeserializeOwned>(path: &str, file: &web_sys::File) -> ApiResult<T> {
    upload_with("POST", path, file).await
}

pub async fn upload_with<T: DeserializeOwned>(method: &str, path: &str, file: &web_sys::File) -> ApiResult<T> {
    let form = web_sys::FormData::new().map_err(ApiError::network)?;
    form.append_with_blob_and_filename("file", file, &file.name()).map_err(ApiError::network)?;
    parse(fetch(method, path, Body::Form(form), None).await?).await
}
