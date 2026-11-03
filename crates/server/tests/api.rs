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
    let data_dir = std::env::temp_dir().join(format!("neko-test-{}", neko_server::crypto::random_token()));
    std::fs::create_dir_all(&data_dir).unwrap();
    let config = Config {
        bind: "127.0.0.1:0".parse().unwrap(),
        data_dir,
        web_dir: PathBuf::from("does-not-exist"),
        allow_signup,
        secure_cookies: false,
        // The mock web page lives on 127.0.0.1.
        fetch_allow_private: true,
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

/// Provider + web page + MCP server in one mock. The "model" decides what to do from the last
/// message: call fetch_url, call the first MCP tool, describe an image, or report a tool result.
async fn mock_tool_world() -> SocketAddr {
    use axum::{http::HeaderMap, response::IntoResponse};

    fn sse(events: Vec<Value>) -> axum::response::Response {
        let mut body = String::new();
        for e in events {
            body.push_str(&format!("data: {e}\n\n"));
        }
        body.push_str("data: [DONE]\n\n");
        ([("content-type", "text/event-stream")], body).into_response()
    }
    fn text(t: &str) -> Value {
        json!({"choices":[{"delta":{"content": t}}]})
    }
    fn tool_call(name: &str, args: Value) -> Vec<Value> {
        let args = args.to_string();
        let (a, b) = args.split_at(args.len() / 2);
        vec![
            text("Let me check. "),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_x1","type":"function","function":{"name":name,"arguments":a}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":b}}]}}]}),
            json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
        ]
    }

    async fn chat(headers: HeaderMap, Json(body): Json<Value>) -> axum::response::Response {
        let host = headers["host"].to_str().unwrap().to_string();
        let msgs = body["messages"].as_array().unwrap();
        let last = msgs.last().unwrap();
        let tools: Vec<String> = body["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| t["function"]["name"].as_str().unwrap().to_string())
            .collect();
        if last["role"] == "tool" {
            let content = last["content"].as_str().unwrap();
            let snippet: String = content.lines().filter(|l| l.contains("Purrfect") || l.contains("pong")).collect();
            return sse(vec![text(&format!("Tool said: {snippet}"))]);
        }
        if let Some(parts) = last["content"].as_array() {
            let has_image = parts.iter().any(|p| p["type"] == "image_url");
            return sse(vec![text(if has_image { "I see an image" } else { "no image" })]);
        }
        let content = last["content"].as_str().unwrap_or_default();
        if content.contains("fetch") && tools.iter().any(|t| t == "fetch_url") {
            return sse(tool_call("fetch_url", json!({"url": format!("http://{host}/page")})));
        }
        if content.contains("mcp") {
            if let Some(t) = tools.iter().find(|t| t.starts_with("mcp")) {
                return sse(tool_call(t, json!({"text": "ping"})));
            }
        }
        sse(vec![text(&format!("tools: {}", tools.join(",")))])
    }

    async fn page() -> impl IntoResponse {
        ([("content-type", "text/html")], "<html><head><title>Cat facts</title></head><body><nav>menu</nav><p>Purrfect fact: cats sleep 16h.</p><script>x()</script></body></html>")
    }

    async fn mcp(headers: HeaderMap, Json(req): Json<Value>) -> axum::response::Response {
        let id = req["id"].clone();
        let method = req["method"].as_str().unwrap_or_default();
        if method == "initialize" {
            let reply = json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"mock"}}});
            return ([("mcp-session-id", "sess-1")], Json(reply)).into_response();
        }
        assert_eq!(headers["mcp-session-id"], "sess-1", "session id must be echoed");
        assert_eq!(headers["authorization"], "Bearer mcp-secret", "custom header must be sent");
        match method {
            "notifications/initialized" => axum::http::StatusCode::ACCEPTED.into_response(),
            "tools/list" => Json(json!({"jsonrpc":"2.0","id":id,"result":{"tools":[
                {"name":"echo","description":"Echo text back","inputSchema":{"$schema":"x","type":"object","properties":{"text":{"type":"string"}}}}
            ]}})).into_response(),
            // Reply as an SSE stream to exercise that path.
            "tools/call" => {
                let t = req["params"]["arguments"]["text"].as_str().unwrap_or_default();
                let msg = json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":format!("{t} pong")}]}});
                ([("content-type", "text/event-stream")], format!("data: {msg}\n\n")).into_response()
            }
            _ => axum::http::StatusCode::BAD_REQUEST.into_response(),
        }
    }

    serve(Router::new().route("/v1/chat/completions", post(chat)).route("/page", get(page)).route("/mcp", post(mcp)))
        .await
}

fn deltas(events: &[StreamEvent]) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Delta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn tools_mcp_and_uploads() {
    use reqwest::Method as M;
    let world = mock_tool_world().await;
    let (mut c, code) = start_app(false).await;
    assert_eq!(register(&mut c, "neko", Some(&code)).await, 200);
    let (_, p) = c
        .json::<Provider>(
            M::POST,
            "/providers",
            json!({"name": "Mock", "kind": "open_ai_compatible", "base_url": format!("http://{world}/v1")}),
        )
        .await;
    let p = p.unwrap();
    let (_, conv) = c.json::<Conversation>(M::POST, "/conversations", json!({})).await;
    let conv = conv.unwrap();
    let path = format!("/conversations/{}/messages", conv.id);

    // 1. fetch_url tool round trip.
    let events = c
        .stream(
            &path,
            json!({"content": "please fetch it", "provider_id": p.id, "model": "m", "tools": {"fetch_url": true}}),
        )
        .await;
    let start = events
        .iter()
        .find_map(|e| match e {
            StreamEvent::ToolStart { call } => Some(call.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!((start.name.as_str(), start.label.as_str(), start.done), ("fetch_url", "Read web page", false));
    let end = events
        .iter()
        .find_map(|e| match e {
            StreamEvent::ToolEnd { call } => Some(call.clone()),
            _ => None,
        })
        .unwrap();
    assert!(end.done && !end.is_error, "{end:?}");
    assert!(
        end.result.contains("Title: Cat facts") && end.result.contains("UNTRUSTED") && !end.result.contains("menu")
    );
    assert_eq!(deltas(&events), "Let me check. \n\nTool said: Purrfect fact: cats sleep 16h.");
    let Some(StreamEvent::Done { message }) = events.last() else { panic!() };
    assert_eq!(message.tool_calls.len(), 1);

    // Tool use is stored and comes back with the conversation.
    let (_, saved) = c.json::<Conversation>(M::GET, &format!("/conversations/{}", conv.id), json!(null)).await;
    assert_eq!(saved.unwrap().messages[1].tool_calls[0].name, "fetch_url");

    // 2. MCP server over Streamable HTTP, with an encrypted custom header.
    let (s, server) = c
        .json::<McpServer>(
            M::POST,
            "/mcp",
            json!({
                "name": "Mock MCP", "transport": "http", "url": format!("http://{world}/mcp"),
                "headers": [["Authorization", "Bearer mcp-secret"]]
            }),
        )
        .await;
    assert_eq!(s, 200);
    let server = server.unwrap();
    assert_eq!(server.header_names, vec!["Authorization"]);
    let raw = c.req(M::GET, "/mcp").send().await.unwrap().text().await.unwrap();
    assert!(!raw.contains("mcp-secret"), "header values must never be returned");
    let (_, test) = c.json::<McpTestResult>(M::POST, &format!("/mcp/{}/test", server.id), json!(null)).await;
    assert_eq!(test.unwrap().tools[0].name, "echo");
    let events = c
        .stream(
            &path,
            json!({"content": "use mcp", "provider_id": p.id, "model": "m", "tools": {"mcp_servers": [server.id]}}),
        )
        .await;
    let end = events
        .iter()
        .find_map(|e| match e {
            StreamEvent::ToolEnd { call } => Some(call.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!((end.label.as_str(), end.result.as_str()), ("Mock MCP › echo", "ping pong"));
    assert!(deltas(&events).ends_with("Tool said: ping pong"));

    // Non-admins can't add local-command servers.
    let (s, _) = c.json::<Value>(M::POST, "/mcp", json!({"name": "x", "transport": "stdio", "command": ""})).await;
    assert_eq!(s, 400, "admin gets validation, not a permission error");

    // An unreachable MCP server shows up as a warning card instead of failing the answer.
    let (_, dead) = c
        .json::<McpServer>(
            M::POST,
            "/mcp",
            json!({"name": "Dead", "transport": "http", "url": "http://127.0.0.1:9/mcp"}),
        )
        .await;
    let events = c
        .stream(&path, json!({"content": "hello", "provider_id": p.id, "model": "m", "tools": {"mcp_servers": [dead.unwrap().id]}}))
        .await;
    assert!(events.iter().any(|e| matches!(e, StreamEvent::ToolEnd { call } if call.is_error && call.label == "Dead")));
    assert!(matches!(events.last(), Some(StreamEvent::Done { message }) if message.error.is_none()));

    // 3. Image upload goes to the model as an image part.
    let png: Vec<u8> = [&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a][..], &[0u8; 32]].concat();
    let form =
        reqwest::multipart::Form::new().part("file", reqwest::multipart::Part::bytes(png.clone()).file_name("cat.png"));
    let resp = c.req(M::POST, "/uploads").multipart(form).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let att: Attachment = resp.json().await.unwrap();
    assert_eq!((att.kind, att.mime.as_str()), (AttachmentKind::Image, "image/png"));
    let events = c
        .stream(&path, json!({"content": "what is this", "provider_id": p.id, "model": "m", "attachments": [att.id]}))
        .await;
    assert!(matches!(&events[0], StreamEvent::Start { user_message: Some(m), .. } if m.attachments.len() == 1));
    assert_eq!(deltas(&events), "I see an image");
    let got = c.req(M::GET, &format!("/uploads/{}", att.id)).send().await.unwrap();
    assert_eq!(got.headers()["content-type"], "image/png");
    assert_eq!(got.bytes().await.unwrap().to_vec(), png);
    // Re-using an already-sent attachment is refused.
    let (s, _) = c
        .json::<Value>(
            M::POST,
            &path,
            json!({"content": "again", "provider_id": p.id, "model": "m", "attachments": [att.id]}),
        )
        .await;
    assert_eq!(s, 400);

    // Unsupported files are rejected; text files are accepted.
    let bad = reqwest::multipart::Form::new()
        .part("file", reqwest::multipart::Part::bytes(vec![0u8, 1, 2, 255]).file_name("x.bin"));
    assert_eq!(c.req(M::POST, "/uploads").multipart(bad).send().await.unwrap().status(), 400);
    let txt = reqwest::multipart::Form::new()
        .part("file", reqwest::multipart::Part::bytes(b"hello".to_vec()).file_name("../a.md"));
    let att2: Attachment = c.req(M::POST, "/uploads").multipart(txt).send().await.unwrap().json().await.unwrap();
    assert_eq!((att2.name.as_str(), att2.kind), ("a.md", AttachmentKind::Text));
    assert_eq!(c.req(M::DELETE, &format!("/uploads/{}", att2.id)).send().await.unwrap().status(), 204);

    // 4. Search settings keep the key server-side.
    let (_, ss) = c
        .json::<SearchSettings>(M::PUT, "/tools/search", json!({"backend": "brave", "api_key": "brv-1234567890"}))
        .await;
    let ss = ss.unwrap();
    assert!(ss.has_key && ss.key_hint == "…7890" && ss.backend == SearchBackend::Brave);

    // Deleting the chat removes its files.
    assert_eq!(c.req(M::DELETE, &format!("/conversations/{}", conv.id)).send().await.unwrap().status(), 204);
    assert_eq!(c.req(M::GET, &format!("/uploads/{}", att.id)).send().await.unwrap().status(), 404);
}
