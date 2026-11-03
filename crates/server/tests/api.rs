//! End-to-end API tests against mock OpenAI-compatible and Anthropic providers.

use std::{net::SocketAddr, path::PathBuf};

use axum::{
    routing::{get, post},
    Json, Router,
};
use neko_server::{crypto::Vault, db::Db, AppState, Config};
use neko_shared::*;
use serde_json::{json, Value};

async fn serve(app: Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

/// A fake provider that echoes the last user message back in three chunks.
async fn mock_provider() -> SocketAddr {
    async fn openai_chat(Json(body): Json<Value>) -> axum::response::Response {
        use axum::response::IntoResponse;
        let last = body["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap().to_string();
        if last.contains("explode") {
            return (axum::http::StatusCode::UNAUTHORIZED, Json(json!({"error": {"message": "Invalid API key"}})))
                .into_response();
        }
        let system = body["messages"][0]["role"] == "system";
        let mut sse = String::new();
        for part in ["Meow: ", last.as_str(), if system { " (sys)" } else { "" }] {
            sse.push_str(&format!("data: {}\n\n", json!({"choices":[{"delta":{"content": part}}]})));
        }
        sse.push_str("data: [DONE]\n\n");
        ([("content-type", "text/event-stream")], sse).into_response()
    }
    async fn anthropic_chat(
        headers: axum::http::HeaderMap,
        Json(body): Json<Value>,
    ) -> impl axum::response::IntoResponse {
        assert_eq!(headers["x-api-key"], "sk-ant-test-key");
        assert_eq!(headers["anthropic-version"], "2023-06-01");
        assert!(body["max_tokens"].as_u64().unwrap() > 0);
        let sse = "event: message_start\ndata: {\"type\":\"message_start\"}\n\n\
                   event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Purr \"}}\n\n\
                   event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"from Claude\"}}\n\n\
                   event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
        ([("content-type", "text/event-stream")], sse)
    }
    let app = Router::new()
        .route("/v1/chat/completions", post(openai_chat))
        .route("/v1/models", get(|| async { Json(json!({"data":[{"id":"mock-b"},{"id":"mock-a"}]})) }))
        .route("/v1/messages", post(anthropic_chat));
    serve(app).await
}

struct Client {
    base: String,
    http: reqwest::Client,
    cookie: Option<String>,
}

impl Client {
    fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let r = self.http.request(method, format!("{}{path}", self.base));
        match &self.cookie {
            Some(c) => r.header("cookie", c),
            None => r,
        }
    }
    async fn json<T: serde::de::DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Value,
    ) -> (u16, Option<T>) {
        let resp = self.req(method, path).json(&body).send().await.unwrap();
        let status = resp.status().as_u16();
        (status, resp.json().await.ok())
    }
    /// Sends a chat request and collects the SSE events.
    async fn stream(&self, path: &str, body: Value) -> Vec<StreamEvent> {
        let resp = self.req(reqwest::Method::POST, path).json(&body).send().await.unwrap();
        assert_eq!(resp.status(), 200, "stream request failed");
        let bytes = resp.bytes().await.unwrap();
        let mut p = sse::SseParser::new();
        p.push(&bytes).into_iter().map(|e| serde_json::from_str(&e.data).unwrap()).collect()
    }
}

async fn start_app(allow_signup: bool) -> (Client, String) {
    let config = Config {
        bind: "127.0.0.1:0".parse().unwrap(),
        data_dir: PathBuf::from("unused"),
        web_dir: PathBuf::from("does-not-exist"),
        allow_signup,
        secure_cookies: false,
    };
    let state = AppState::new(config, Db::open_in_memory().unwrap(), Vault::new([1; 32]), Some("SETUP123".into()));
    let addr = serve(neko_server::router(state)).await;
    (Client { base: format!("http://{addr}/api"), http: reqwest::Client::new(), cookie: None }, "SETUP123".into())
}

async fn register(c: &mut Client, name: &str, code: Option<&str>) -> u16 {
    let resp = c
        .req(reqwest::Method::POST, "/auth/register")
        .json(&json!({"username": name, "password": "correct horse", "setup_code": code}))
        .send()
        .await
        .unwrap();
    if let Some(sc) = resp.headers().get("set-cookie") {
        let sc = sc.to_str().unwrap();
        assert!(sc.contains("HttpOnly") && sc.contains("SameSite=Strict"));
        c.cookie = Some(sc.split(';').next().unwrap().to_string());
    }
    resp.status().as_u16()
}

#[tokio::test]
async fn full_chat_flow() {
    use reqwest::Method as M;
    let mock = mock_provider().await;
    let (mut c, code) = start_app(false).await;

    // Unauthenticated access is rejected; first sign-up needs the setup code.
    assert_eq!(c.req(M::GET, "/conversations").send().await.unwrap().status(), 401);
    let (_, st) = c.json::<AuthStatus>(M::GET, "/auth/status", json!(null)).await;
    assert!(st.unwrap().needs_setup);
    assert_eq!(register(&mut c, "neko", Some("WRONG")).await, 403);
    assert_eq!(register(&mut c, "neko", Some(&code.to_lowercase())).await, 200);
    let (_, me) = c.json::<User>(M::GET, "/auth/me", json!(null)).await;
    assert_eq!(me.unwrap().role, Role::Admin);

    // Sign-up is closed afterwards.
    let mut other = Client { base: c.base.clone(), http: reqwest::Client::new(), cookie: None };
    assert_eq!(register(&mut other, "intruder", Some(&code)).await, 403);

    // Providers: key is stored but never returned.
    let (s, p) = c
        .json::<Provider>(M::POST, "/providers", json!({
            "name": "Mock", "kind": "open_ai_compatible", "base_url": format!("http://{mock}/v1/"), "api_key": "sk-test-abcdef1234"
        }))
        .await;
    assert_eq!(s, 200);
    let p = p.unwrap();
    assert!(p.has_key);
    assert_eq!(p.key_hint, "…1234");
    assert_eq!(p.base_url, format!("http://{mock}/v1"));
    let raw = c.req(M::GET, "/providers").send().await.unwrap().text().await.unwrap();
    assert!(!raw.contains("sk-test"));
    let (_, models) = c.json::<ModelList>(M::GET, &format!("/providers/{}/models", p.id), json!(null)).await;
    assert_eq!(models.unwrap().models, vec!["mock-a", "mock-b"]);

    // Chat with streaming.
    let (_, conv) = c.json::<Conversation>(M::POST, "/conversations", json!({})).await;
    let conv = conv.unwrap();
    let events = c
        .stream(
            &format!("/conversations/{}/messages", conv.id),
            json!({
                "content": "hello kitty", "provider_id": p.id, "model": "mock-a",
                "params": {"system_prompt": "be cute", "temperature": 0.7}
            }),
        )
        .await;
    assert!(matches!(&events[0], StreamEvent::Start { user_message: Some(m), .. } if m.content == "hello kitty"));
    let text: String = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Delta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "Meow: hello kitty (sys)");
    assert!(
        matches!(events.last(), Some(StreamEvent::Done { message }) if message.content == text && message.error.is_none())
    );

    let (_, saved) = c.json::<Conversation>(M::GET, &format!("/conversations/{}", conv.id), json!(null)).await;
    let saved = saved.unwrap();
    assert_eq!(saved.title, "hello kitty");
    assert_eq!(saved.messages.len(), 2);
    assert_eq!(saved.params.system_prompt, "be cute");
    assert_eq!(saved.model.as_deref(), Some("mock-a"));

    // Switch to a second provider mid-chat (Anthropic format) and regenerate.
    let (_, ant) = c
        .json::<Provider>(M::POST, "/providers", json!({
            "name": "Claude", "kind": "anthropic", "base_url": format!("http://{mock}"), "api_key": "sk-ant-test-key"
        }))
        .await;
    let ant = ant.unwrap();
    let events = c
        .stream(&format!("/conversations/{}/regenerate", conv.id), json!({"provider_id": ant.id, "model": "claude-x"}))
        .await;
    assert!(matches!(&events[0], StreamEvent::Start { user_message: None, .. }));
    assert!(
        matches!(events.last(), Some(StreamEvent::Done { message }) if message.content == "Purr from Claude" && message.model.as_deref() == Some("claude-x"))
    );
    let (_, saved) = c.json::<Conversation>(M::GET, &format!("/conversations/{}", conv.id), json!(null)).await;
    assert_eq!(saved.unwrap().messages.len(), 2);

    // Provider errors are reported in-stream and stored on the message.
    let events = c
        .stream(
            &format!("/conversations/{}/messages", conv.id),
            json!({"content": "explode", "provider_id": p.id, "model": "mock-a"}),
        )
        .await;
    assert!(events.iter().any(|e| matches!(e, StreamEvent::Error { message } if message.contains("Invalid API key"))));
    assert!(matches!(events.last(), Some(StreamEvent::Done { message }) if message.error.is_some()));

    // Edit-and-resend replaces the failed exchange.
    let (_, saved) = c.json::<Conversation>(M::GET, &format!("/conversations/{}", conv.id), json!(null)).await;
    let failed_user_msg = saved.unwrap().messages[2].id;
    c.stream(
        &format!("/conversations/{}/messages", conv.id),
        json!({
            "content": "fixed question", "provider_id": p.id, "model": "mock-a", "replace_from": failed_user_msg
        }),
    )
    .await;
    let (_, saved) = c.json::<Conversation>(M::GET, &format!("/conversations/{}", conv.id), json!(null)).await;
    let msgs = saved.unwrap().messages;
    assert_eq!(msgs.len(), 4);
    assert_eq!(msgs[3].content, "Meow: fixed question");

    // Search finds message text.
    let (_, hits) = c.json::<Vec<SearchHit>>(M::GET, "/search?q=fixed", json!(null)).await;
    assert_eq!(hits.unwrap()[0].conversation_id, conv.id);

    // Validation.
    let (s, _) = c
        .json::<Value>(M::PATCH, &format!("/conversations/{}", conv.id), json!({"params": {"temperature": 5.0}}))
        .await;
    assert_eq!(s, 400);
    let (s, _) = c
        .json::<Value>(
            M::POST,
            &format!("/conversations/{}/messages", conv.id),
            json!({"content": "  ", "provider_id": p.id, "model": "m"}),
        )
        .await;
    assert_eq!(s, 400);

    // Devices + logout.
    let (_, dev) = c.json::<DevicesInfo>(M::GET, "/devices", json!(null)).await;
    let dev = dev.unwrap();
    assert_eq!(dev.sessions.len(), 1);
    assert!(dev.sessions[0].current);
    assert_eq!(c.req(M::POST, "/auth/logout").send().await.unwrap().status(), 204);
    assert_eq!(c.req(M::GET, "/auth/me").send().await.unwrap().status(), 401);
}

#[tokio::test]
async fn users_are_isolated_and_login_is_rate_limited() {
    use reqwest::Method as M;
    let (mut admin, code) = start_app(true).await;
    assert_eq!(register(&mut admin, "admin", Some(&code)).await, 200);
    let (_, conv) = admin.json::<Conversation>(M::POST, "/conversations", json!({"title": "secret plans"})).await;
    let conv = conv.unwrap();

    let mut bob = Client { base: admin.base.clone(), http: reqwest::Client::new(), cookie: None };
    assert_eq!(register(&mut bob, "bob", None).await, 200);
    let (_, me) = bob.json::<User>(M::GET, "/auth/me", json!(null)).await;
    assert_eq!(me.unwrap().role, Role::User);
    assert_eq!(register(&mut bob, "BOB", None).await, 409);
    assert_eq!(bob.req(M::GET, &format!("/conversations/{}", conv.id)).send().await.unwrap().status(), 404);
    assert_eq!(bob.req(M::DELETE, &format!("/conversations/{}", conv.id)).send().await.unwrap().status(), 404);
    let (_, hits) = bob.json::<Vec<SearchHit>>(M::GET, "/search?q=secret", json!(null)).await;
    assert!(hits.unwrap().is_empty());

    let anon = Client { base: admin.base.clone(), http: reqwest::Client::new(), cookie: None };
    for _ in 0..5 {
        let (s, _) =
            anon.json::<Value>(M::POST, "/auth/login", json!({"username": "bob", "password": "wrong-password"})).await;
        assert_eq!(s, 401);
    }
    let (s, _) =
        anon.json::<Value>(M::POST, "/auth/login", json!({"username": "bob", "password": "correct horse"})).await;
    assert_eq!(s, 429);
    let (s, _) =
        anon.json::<Value>(M::POST, "/auth/login", json!({"username": "admin", "password": "correct horse"})).await;
    assert_eq!(s, 200);
    assert_eq!(anon.req(M::GET, "/nope").send().await.unwrap().status(), 404);
}
