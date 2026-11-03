//! Minimal MCP (Model Context Protocol) client: Streamable HTTP and stdio transports,
//! `initialize`, `tools/list` and `tools/call`.

use std::{
    collections::HashMap,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use neko_shared::{sse::SseParser, McpTransport};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{oneshot, Mutex},
};

const PROTOCOL_VERSION: &str = "2025-06-18";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
const TOOLS_CACHE_TTL: Duration = Duration::from_secs(60);

/// Decrypted connection settings for one server.
#[derive(Debug, Clone, PartialEq)]
pub struct McpConfig {
    pub id: i64,
    pub name: String,
    pub transport: McpTransport,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Clone, Default)]
pub struct McpManager {
    clients: Arc<Mutex<HashMap<i64, Arc<McpClient>>>>,
}

impl McpManager {
    async fn client(&self, http: &reqwest::Client, cfg: &McpConfig) -> Result<Arc<McpClient>, String> {
        let mut clients = self.clients.lock().await;
        if let Some(c) = clients.get(&cfg.id) {
            if c.config == *cfg && c.is_alive() {
                return Ok(c.clone());
            }
        }
        if let Some(old) = clients.remove(&cfg.id) {
            old.shutdown().await;
        }
        let c = Arc::new(McpClient::connect(http, cfg.clone()).await?);
        clients.insert(cfg.id, c.clone());
        Ok(c)
    }

    /// Runs `f` against a connected client, reconnecting once if the session went stale.
    async fn with_client<T, F, Fut>(&self, http: &reqwest::Client, cfg: &McpConfig, f: F) -> Result<T, String>
    where
        F: Fn(Arc<McpClient>) -> Fut,
        Fut: std::future::Future<Output = Result<T, McpError>>,
    {
        let c = self.client(http, cfg).await?;
        match f(c).await {
            Ok(v) => Ok(v),
            Err(McpError::SessionExpired) => {
                self.forget(cfg.id).await;
                let c = self.client(http, cfg).await?;
                f(c).await.map_err(|e| e.to_string())
            }
            Err(e) => Err(e.to_string()),
        }
    }

    pub async fn list_tools(&self, http: &reqwest::Client, cfg: &McpConfig) -> Result<Vec<McpTool>, String> {
        self.with_client(http, cfg, |c| async move { c.list_tools().await }).await
    }

    pub async fn call_tool(
        &self,
        http: &reqwest::Client,
        cfg: &McpConfig,
        name: &str,
        args: Value,
    ) -> Result<(String, bool), String> {
        self.with_client(http, cfg, |c| {
            let args = args.clone();
            async move {
                let result = c.request("tools/call", json!({ "name": name, "arguments": args })).await?;
                Ok(render_tool_result(&result))
            }
        })
        .await
    }

    /// Disconnects a server (e.g. after it was edited or deleted). Stops a local process.
    pub async fn forget(&self, id: i64) {
        if let Some(c) = self.clients.lock().await.remove(&id) {
            c.shutdown().await;
        }
    }
}

#[derive(Debug)]
enum McpError {
    SessionExpired,
    Other(String),
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpError::SessionExpired => f.write_str("MCP session expired"),
            McpError::Other(s) => f.write_str(s),
        }
    }
}

impl From<String> for McpError {
    fn from(s: String) -> Self {
        McpError::Other(s)
    }
}

type Pending = Arc<std::sync::Mutex<HashMap<i64, oneshot::Sender<Value>>>>;

enum Transport {
    Http { http: reqwest::Client, session: Mutex<Option<String>>, protocol: Mutex<String> },
    Stdio { stdin: Arc<Mutex<ChildStdin>>, pending: Pending, child: Mutex<Child>, alive: Arc<AtomicBool> },
}

struct McpClient {
    config: McpConfig,
    transport: Transport,
    next_id: AtomicI64,
    tools: Mutex<Option<(Instant, Vec<McpTool>)>>,
}

impl McpClient {
    async fn connect(http: &reqwest::Client, config: McpConfig) -> Result<Self, String> {
        let transport = match config.transport {
            McpTransport::Http => {
                if !(config.url.starts_with("http://") || config.url.starts_with("https://")) {
                    return Err("MCP server URL must start with http:// or https://".into());
                }
                Transport::Http {
                    http: http.clone(),
                    session: Mutex::new(None),
                    protocol: Mutex::new(PROTOCOL_VERSION.into()),
                }
            }
            McpTransport::Stdio => spawn_stdio(&config)?,
        };
        let client = McpClient { config, transport, next_id: AtomicI64::new(1), tools: Mutex::new(None) };
        let init = client
            .request(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": { "name": "neko-ai", "version": env!("CARGO_PKG_VERSION") }
                }),
            )
            .await
            .map_err(|e| format!("Could not start an MCP session with “{}”: {e}", client.config.name))?;
        if let Transport::Http { protocol, .. } = &client.transport {
            if let Some(v) = init["protocolVersion"].as_str() {
                *protocol.lock().await = v.to_string();
            }
        }
        client.notify("notifications/initialized").await;
        Ok(client)
    }

    fn is_alive(&self) -> bool {
        match &self.transport {
            Transport::Http { .. } => true,
            Transport::Stdio { alive, .. } => alive.load(Ordering::Relaxed),
        }
    }

    async fn shutdown(&self) {
        match &self.transport {
            Transport::Http { http, session, .. } => {
                // Politely end the session; servers may ignore this.
                if let Some(id) = session.lock().await.clone() {
                    let _ = http
                        .delete(&self.config.url)
                        .header("mcp-session-id", id)
                        .timeout(Duration::from_secs(5))
                        .send()
                        .await;
                }
            }
            Transport::Stdio { child, alive, .. } => {
                alive.store(false, Ordering::Relaxed);
                let _ = child.lock().await.kill().await;
            }
        }
    }

    async fn list_tools(&self) -> Result<Vec<McpTool>, McpError> {
        if let Some((at, tools)) = self.tools.lock().await.as_ref() {
            if at.elapsed() < TOOLS_CACHE_TTL {
                return Ok(tools.clone());
            }
        }
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..20 {
            let params = match &cursor {
                Some(c) => json!({ "cursor": c }),
                None => json!({}),
            };
            let page = self.request("tools/list", params).await?;
            for t in page["tools"].as_array().into_iter().flatten() {
                let Some(name) = t["name"].as_str() else { continue };
                tools.push(McpTool {
                    name: name.to_string(),
                    description: t["description"].as_str().unwrap_or_default().to_string(),
                    input_schema: t.get("inputSchema").cloned().unwrap_or_else(|| json!({ "type": "object" })),
                });
            }
            cursor = page["nextCursor"].as_str().filter(|c| !c.is_empty()).map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        *self.tools.lock().await = Some((Instant::now(), tools.clone()));
        Ok(tools)
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, McpError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let reply = match &self.transport {
            Transport::Http { http, session, protocol } => {
                let mut req = http
                    .post(&self.config.url)
                    .header("accept", "application/json, text/event-stream")
                    .header("mcp-protocol-version", protocol.lock().await.clone())
                    .timeout(REQUEST_TIMEOUT)
                    .json(&msg);
                for (k, v) in &self.config.headers {
                    req = req.header(k, v);
                }
                let had_session = session.lock().await.clone();
                if let Some(s) = &had_session {
                    req = req.header("mcp-session-id", s);
                }
                let resp = req.send().await.map_err(|e| format!("MCP request failed: {e}"))?;
                if resp.status() == reqwest::StatusCode::NOT_FOUND && had_session.is_some() {
                    return Err(McpError::SessionExpired);
                }
                if !resp.status().is_success() {
                    let status = resp.status();
                    let body: String = resp.text().await.unwrap_or_default().chars().take(300).collect();
                    return Err(format!("MCP server answered {status}: {body}").into());
                }
                if let Some(s) = resp.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()) {
                    *session.lock().await = Some(s.to_string());
                }
                read_http_reply(resp, id).await?
            }
            Transport::Stdio { stdin, pending, alive, .. } => {
                if !alive.load(Ordering::Relaxed) {
                    return Err(McpError::Other("The MCP process has exited".into()));
                }
                let (tx, rx) = oneshot::channel();
                pending.lock().unwrap().insert(id, tx);
                let mut line = msg.to_string();
                line.push('\n');
                let write = async {
                    let mut w = stdin.lock().await;
                    w.write_all(line.as_bytes()).await?;
                    w.flush().await
                };
                if let Err(e) = write.await {
                    pending.lock().unwrap().remove(&id);
                    return Err(format!("Could not talk to the MCP process: {e}").into());
                }
                match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
                    Ok(Ok(v)) => v,
                    Ok(Err(_)) => return Err("The MCP process exited".to_string().into()),
                    Err(_) => {
                        pending.lock().unwrap().remove(&id);
                        return Err("The MCP server took too long to answer".to_string().into());
                    }
                }
            }
        };
        if let Some(err) = reply.get("error").filter(|e| !e.is_null()) {
            let msg = err["message"].as_str().unwrap_or("unknown error");
            return Err(format!("MCP error: {msg}").into());
        }
        Ok(reply.get("result").cloned().unwrap_or(Value::Null))
    }

    async fn notify(&self, method: &str) {
        let msg = json!({ "jsonrpc": "2.0", "method": method });
        match &self.transport {
            Transport::Http { http, session, protocol } => {
                let mut req = http
                    .post(&self.config.url)
                    .header("accept", "application/json, text/event-stream")
                    .header("mcp-protocol-version", protocol.lock().await.clone())
                    .timeout(Duration::from_secs(15))
                    .json(&msg);
                for (k, v) in &self.config.headers {
                    req = req.header(k, v);
                }
                if let Some(s) = session.lock().await.clone() {
                    req = req.header("mcp-session-id", s);
                }
                let _ = req.send().await;
            }
            Transport::Stdio { stdin, .. } => {
                let mut line = msg.to_string();
                line.push('\n');
                let mut w = stdin.lock().await;
                let _ = w.write_all(line.as_bytes()).await;
                let _ = w.flush().await;
            }
        }
    }
}

/// A Streamable HTTP reply is either plain JSON or an SSE stream carrying our response.
async fn read_http_reply(resp: reqwest::Response, id: i64) -> Result<Value, McpError> {
    let ct = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    if !ct.contains("text/event-stream") {
        let v: Value = resp.json().await.map_err(|e| format!("Bad MCP response: {e}"))?;
        // Batched replies are allowed by JSON-RPC.
        if let Some(arr) = v.as_array() {
            return arr
                .iter()
                .find(|m| m["id"] == id)
                .cloned()
                .ok_or_else(|| "MCP reply had no matching id".to_string().into());
        }
        return Ok(v);
    }
    let mut parser = SseParser::new();
    let mut body = resp.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|e| format!("MCP stream interrupted: {e}"))?;
        for ev in parser.push(&chunk) {
            if let Ok(v) = serde_json::from_str::<Value>(&ev.data) {
                if v["id"] == id && v.get("method").is_none() {
                    return Ok(v);
                }
            }
        }
    }
    Err("MCP stream ended without a reply".to_string().into())
}

fn spawn_stdio(cfg: &McpConfig) -> Result<Transport, String> {
    if cfg.command.trim().is_empty() {
        return Err("Set the command to run".into());
    }
    let mut child = Command::new(cfg.command.trim())
        .args(&cfg.args)
        .envs(cfg.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("Could not start `{}`: {e}", cfg.command))?;
    let stdin = Arc::new(Mutex::new(child.stdin.take().ok_or("no stdin")?));
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;
    let pending: Pending = Arc::default();
    let alive = Arc::new(AtomicBool::new(true));

    let (p, a, w, name) = (pending.clone(), alive.clone(), stdin.clone(), cfg.name.clone());
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            match (v.get("id"), v.get("method")) {
                // Response to one of our requests.
                (Some(id), None) => {
                    if let Some(tx) = id.as_i64().and_then(|id| p.lock().unwrap().remove(&id)) {
                        let _ = tx.send(v);
                    }
                }
                // Request from the server (ping, roots, sampling…): answer ping, decline the rest.
                (Some(id), Some(method)) => {
                    let reply = if method == "ping" {
                        json!({ "jsonrpc": "2.0", "id": id, "result": {} })
                    } else {
                        json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "Not supported by Neko AI" } })
                    };
                    let mut out = reply.to_string();
                    out.push('\n');
                    let mut w = w.lock().await;
                    let _ = w.write_all(out.as_bytes()).await;
                    let _ = w.flush().await;
                }
                _ => {} // notifications
            }
        }
        a.store(false, Ordering::Relaxed);
        p.lock().unwrap().clear(); // wakes waiters with an error
        tracing::info!(server = %name, "MCP process exited");
    });
    let name = cfg.name.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::debug!(server = %name, "mcp stderr: {line}");
        }
    });
    Ok(Transport::Stdio { stdin, pending, child: Mutex::new(child), alive })
}

/// Flattens a `tools/call` result into text for the model.
pub fn render_tool_result(result: &Value) -> (String, bool) {
    let is_error = result["isError"] == true;
    let mut parts: Vec<String> = Vec::new();
    for item in result["content"].as_array().into_iter().flatten() {
        match item["type"].as_str() {
            Some("text") => parts.push(item["text"].as_str().unwrap_or_default().to_string()),
            Some("image") => parts.push(format!("[image: {}]", item["mimeType"].as_str().unwrap_or("image"))),
            Some("audio") => parts.push("[audio]".into()),
            Some("resource") => {
                let r = &item["resource"];
                parts.push(
                    r["text"]
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("[resource: {}]", r["uri"].as_str().unwrap_or("?"))),
                );
            }
            Some("resource_link") => parts.push(format!("[link: {}]", item["uri"].as_str().unwrap_or("?"))),
            _ => {}
        }
    }
    if parts.is_empty() {
        if let Some(sc) = result.get("structuredContent").filter(|v| !v.is_null()) {
            parts.push(sc.to_string());
        }
    }
    let text = if parts.is_empty() { "(no output)".to_string() } else { parts.join("\n") };
    (super::web::truncate(&text, 30_000), is_error)
}

/// Model-facing tool name: `mcp<id>__<tool>`, limited to the characters every provider accepts.
pub fn tool_alias(server_id: i64, tool: &str) -> String {
    let clean: String =
        tool.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    let mut name = format!("mcp{server_id}__{clean}");
    name.truncate(64);
    name
}

/// Providers want an object schema without JSON-Schema meta keys.
pub fn clean_schema(mut schema: Value) -> Value {
    if !schema.is_object() {
        return json!({ "type": "object", "properties": {} });
    }
    if let Some(obj) = schema.as_object_mut() {
        obj.remove("$schema");
        obj.remove("$id");
        obj.entry("type").or_insert(json!("object"));
        obj.entry("properties").or_insert(json!({}));
    }
    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_results() {
        let (t, err) = render_tool_result(&json!({
            "content": [{"type":"text","text":"hello"},{"type":"image","mimeType":"image/png","data":"x"},
                        {"type":"resource","resource":{"uri":"file:///a","text":"body"}}],
            "isError": false
        }));
        assert_eq!(t, "hello\n[image: image/png]\nbody");
        assert!(!err);
        let (t, err) = render_tool_result(&json!({"content": [], "structuredContent": {"n": 1}, "isError": true}));
        assert_eq!(t, "{\"n\":1}");
        assert!(err);
    }

    #[test]
    fn aliases_and_schemas() {
        assert_eq!(tool_alias(3, "read file.v2"), "mcp3__read_file_v2");
        assert_eq!(tool_alias(1, &"x".repeat(100)).len(), 64);
        let s = clean_schema(json!({"$schema": "http://json-schema.org/draft-07/schema#", "properties": {"a": {}}}));
        assert_eq!(s, json!({"type": "object", "properties": {"a": {}}}));
        assert_eq!(clean_schema(json!(null))["type"], "object");
    }

    /// A tiny MCP server written as a shell script, to exercise the stdio transport for real.
    #[tokio::test]
    async fn stdio_round_trip() {
        if !std::path::Path::new("/bin/sh").exists() {
            return;
        }
        let script = r#"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"sh"}}}\n' "$id" ;;
    *'"method":"tools/list"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"tools":[{"name":"echo","description":"Echo","inputSchema":{"type":"object"}}]}}\n' "$id" ;;
    *'"method":"tools/call"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"content":[{"type":"text","text":"meow back"}]}}\n' "$id" ;;
  esac
done"#;
        let cfg = McpConfig {
            id: 1,
            name: "sh".into(),
            transport: McpTransport::Stdio,
            url: String::new(),
            headers: vec![],
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            env: vec![],
        };
        let mgr = McpManager::default();
        let http = reqwest::Client::new();
        let tools = mgr.list_tools(&http, &cfg).await.unwrap();
        assert_eq!(tools[0].name, "echo");
        let (out, err) = mgr.call_tool(&http, &cfg, "echo", json!({})).await.unwrap();
        assert_eq!((out.as_str(), err), ("meow back", false));
        mgr.forget(1).await;
    }
}
