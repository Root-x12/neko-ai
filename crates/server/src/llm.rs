//! Provider adapters. Every provider is turned into one stream of text deltas.

use std::{pin::Pin, time::Duration};

use futures_util::{Stream, StreamExt};
use neko_shared::{sse::SseParser, ChatParams, MsgRole, ProviderKind};
use serde_json::{json, Value};

pub type TextStream = Pin<Box<dyn Stream<Item = Result<String, String>> + Send>>;

/// Everything needed to call a provider (the key is already decrypted).
#[derive(Clone)]
pub struct Endpoint {
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChatTurn {
    pub role: MsgRole,
    pub content: String,
}

const ANTHROPIC_VERSION: &str = "2023-06-01";
const ANTHROPIC_DEFAULT_MAX_TOKENS: u32 = 8192;

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        // Long generations are fine, but a provider that goes silent should not hang forever.
        .read_timeout(Duration::from_secs(300))
        .user_agent(concat!("neko-ai/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("building HTTP client")
}

fn url(base: &str, path: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), path.trim_start_matches('/'))
}

fn authed(ep: &Endpoint, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    let Some(key) = ep.api_key.as_deref().filter(|k| !k.is_empty()) else { return req };
    match ep.kind {
        ProviderKind::OpenAiCompatible => req.bearer_auth(key),
        ProviderKind::Anthropic => req.header("x-api-key", key),
        ProviderKind::Gemini => req.header("x-goog-api-key", key),
    }
}

/// Merges consecutive turns with the same role and drops empty ones, since some providers
/// (Anthropic, Gemini) reject anything but strictly alternating user/assistant turns.
pub fn normalize_turns(turns: &[ChatTurn]) -> Vec<ChatTurn> {
    let mut out: Vec<ChatTurn> = Vec::new();
    for t in turns.iter().filter(|t| !t.content.trim().is_empty()) {
        match out.last_mut() {
            Some(last) if last.role == t.role => {
                last.content.push_str("\n\n");
                last.content.push_str(&t.content);
            }
            _ => out.push(t.clone()),
        }
    }
    while out.first().is_some_and(|t| t.role == MsgRole::Assistant) {
        out.remove(0);
    }
    out
}

pub fn build_request_body(kind: ProviderKind, model: &str, turns: &[ChatTurn], p: &ChatParams) -> Value {
    let system = p.system_prompt.trim();
    match kind {
        ProviderKind::OpenAiCompatible => {
            let mut messages = Vec::new();
            if !system.is_empty() {
                messages.push(json!({ "role": "system", "content": system }));
            }
            messages.extend(turns.iter().map(|t| json!({ "role": t.role.as_str(), "content": t.content })));
            let mut body = json!({ "model": model, "messages": messages, "stream": true });
            if let Some(v) = p.temperature {
                body["temperature"] = json!(v);
            }
            if let Some(v) = p.top_p {
                body["top_p"] = json!(v);
            }
            if let Some(v) = p.max_tokens {
                body["max_tokens"] = json!(v);
            }
            body
        }
        ProviderKind::Anthropic => {
            let messages: Vec<Value> =
                turns.iter().map(|t| json!({ "role": t.role.as_str(), "content": t.content })).collect();
            let mut body = json!({
                "model": model,
                "messages": messages,
                "max_tokens": p.max_tokens.unwrap_or(ANTHROPIC_DEFAULT_MAX_TOKENS),
                "stream": true,
            });
            if !system.is_empty() {
                body["system"] = json!(system);
            }
            if let Some(v) = p.temperature {
                body["temperature"] = json!(v.clamp(0.0, 1.0));
            }
            if let Some(v) = p.top_p {
                body["top_p"] = json!(v);
            }
            body
        }
        ProviderKind::Gemini => {
            let contents: Vec<Value> = turns
                .iter()
                .map(|t| {
                    let role = if t.role == MsgRole::Assistant { "model" } else { "user" };
                    json!({ "role": role, "parts": [{ "text": t.content }] })
                })
                .collect();
            let mut config = json!({});
            if let Some(v) = p.temperature {
                config["temperature"] = json!(v);
            }
            if let Some(v) = p.top_p {
                config["topP"] = json!(v);
            }
            if let Some(v) = p.max_tokens {
                config["maxOutputTokens"] = json!(v);
            }
            let mut body = json!({ "contents": contents, "generationConfig": config });
            if !system.is_empty() {
                body["systemInstruction"] = json!({ "parts": [{ "text": system }] });
            }
            body
        }
    }
}

/// What one SSE event from a provider means for us.
#[derive(Debug, PartialEq)]
pub enum Chunk {
    Text(String),
    Error(String),
    Done,
    Ignore,
}

pub fn parse_event(kind: ProviderKind, event: Option<&str>, data: &str) -> Chunk {
    if data.trim() == "[DONE]" {
        return Chunk::Done;
    }
    let Ok(v) = serde_json::from_str::<Value>(data) else { return Chunk::Ignore };
    if let Some(err) = v.get("error") {
        return Chunk::Error(error_text(err));
    }
    match kind {
        ProviderKind::OpenAiCompatible => {
            let text = v["choices"][0]["delta"]["content"].as_str().unwrap_or_default();
            if text.is_empty() {
                Chunk::Ignore
            } else {
                Chunk::Text(text.to_string())
            }
        }
        ProviderKind::Anthropic => match event.or(v["type"].as_str()) {
            Some("content_block_delta") if v["delta"]["type"] == "text_delta" => {
                Chunk::Text(v["delta"]["text"].as_str().unwrap_or_default().to_string())
            }
            Some("message_stop") => Chunk::Done,
            Some("error") => Chunk::Error(error_text(&v)),
            _ => Chunk::Ignore,
        },
        ProviderKind::Gemini => {
            let text: String = v["candidates"][0]["content"]["parts"]
                .as_array()
                .map(|parts| parts.iter().filter_map(|p| p["text"].as_str()).collect())
                .unwrap_or_default();
            if !text.is_empty() {
                return Chunk::Text(text);
            }
            match v["promptFeedback"]["blockReason"].as_str() {
                Some(reason) => Chunk::Error(format!("Gemini blocked the prompt ({reason})")),
                None => Chunk::Ignore,
            }
        }
    }
}

fn error_text(err: &Value) -> String {
    err["message"]
        .as_str()
        .or_else(|| err["error"]["message"].as_str())
        .or_else(|| err.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| err.to_string())
}

/// Readable message for a non-2xx provider response.
async fn http_error(resp: reqwest::Response) -> String {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<Value>(&body)
        .ok()
        .map(|v| {
            // OpenAI/Anthropic: {"error":{"message"}}; Gemini: [{"error":{...}}] or {"error":{...}}
            let v = if v.is_array() { v[0].clone() } else { v };
            error_text(v.get("error").unwrap_or(&v))
        })
        .unwrap_or_else(|| body.chars().take(300).collect());
    format!("Provider returned {status}: {detail}")
}

fn send_error(e: reqwest::Error) -> String {
    if e.is_connect() {
        format!("Could not connect to the provider: {e}")
    } else if e.is_timeout() {
        "The provider took too long to respond".into()
    } else {
        format!("Request to the provider failed: {e}")
    }
}

pub async fn stream_chat(
    client: &reqwest::Client,
    ep: &Endpoint,
    model: &str,
    turns: &[ChatTurn],
    params: &ChatParams,
) -> Result<TextStream, String> {
    let turns = normalize_turns(turns);
    if turns.is_empty() {
        return Err("Nothing to send".into());
    }
    let body = build_request_body(ep.kind, model, &turns, params);
    let target = match ep.kind {
        ProviderKind::OpenAiCompatible => url(&ep.base_url, "chat/completions"),
        ProviderKind::Anthropic => url(&ep.base_url, "v1/messages"),
        ProviderKind::Gemini => url(
            &ep.base_url,
            &format!("v1beta/models/{}:streamGenerateContent?alt=sse", model.trim_start_matches("models/")),
        ),
    };
    let mut req = authed(ep, client.post(target)).json(&body).header("accept", "text/event-stream");
    if ep.kind == ProviderKind::Anthropic {
        req = req.header("anthropic-version", ANTHROPIC_VERSION);
    }
    let resp = req.send().await.map_err(send_error)?;
    if !resp.status().is_success() {
        return Err(http_error(resp).await);
    }

    let kind = ep.kind;
    let mut bytes = resp.bytes_stream();
    let stream = async_stream(move |tx| async move {
        let mut parser = SseParser::new();
        while let Some(chunk) = bytes.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => return tx.send(Err(format!("Stream interrupted: {e}"))).await,
            };
            for ev in parser.push(&chunk) {
                match parse_event(kind, ev.event.as_deref(), &ev.data) {
                    Chunk::Text(t) => tx.send(Ok(t)).await?,
                    Chunk::Error(e) => return tx.send(Err(e)).await,
                    Chunk::Done => return Ok(()),
                    Chunk::Ignore => {}
                }
            }
        }
        if let Some(ev) = parser.finish() {
            match parse_event(kind, ev.event.as_deref(), &ev.data) {
                Chunk::Text(t) => tx.send(Ok(t)).await?,
                Chunk::Error(e) => tx.send(Err(e)).await?,
                _ => {}
            }
        }
        Ok(())
    });
    Ok(stream)
}

/// Small channel-backed stream builder (avoids pulling in an extra macro crate).
fn async_stream<F, Fut>(f: F) -> TextStream
where
    F: FnOnce(tokio::sync::mpsc::Sender<Result<String, String>>) -> Fut,
    Fut: std::future::Future<Output = Result<(), tokio::sync::mpsc::error::SendError<Result<String, String>>>>
        + Send
        + 'static,
{
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    tokio::spawn(f(tx));
    Box::pin(futures_util::stream::poll_fn(move |cx| rx.poll_recv(cx)))
}

pub async fn list_models(client: &reqwest::Client, ep: &Endpoint) -> Result<Vec<String>, String> {
    let target = match ep.kind {
        ProviderKind::OpenAiCompatible => url(&ep.base_url, "models"),
        ProviderKind::Anthropic => url(&ep.base_url, "v1/models?limit=1000"),
        ProviderKind::Gemini => url(&ep.base_url, "v1beta/models?pageSize=1000"),
    };
    let mut req = authed(ep, client.get(target)).timeout(Duration::from_secs(20));
    if ep.kind == ProviderKind::Anthropic {
        req = req.header("anthropic-version", ANTHROPIC_VERSION);
    }
    let resp = req.send().await.map_err(send_error)?;
    if !resp.status().is_success() {
        return Err(http_error(resp).await);
    }
    let v: Value = resp.json().await.map_err(|e| format!("Unexpected model list: {e}"))?;
    Ok(parse_model_list(ep.kind, &v))
}

pub fn parse_model_list(kind: ProviderKind, v: &Value) -> Vec<String> {
    let mut models: Vec<String> = match kind {
        ProviderKind::Gemini => v["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| {
                m["supportedGenerationMethods"].as_array().is_none_or(|ms| ms.iter().any(|x| x == "generateContent"))
            })
            .filter_map(|m| m["name"].as_str())
            .map(|n| n.trim_start_matches("models/").to_string())
            .collect(),
        // OpenAI-style {"data":[{"id"}]}; Ollama's native {"models":[{"name"}]} as a fallback.
        _ => v["data"]
            .as_array()
            .or_else(|| v["models"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|m| m["id"].as_str().or_else(|| m["name"].as_str()))
            .map(str::to_string)
            .collect(),
    };
    models.sort();
    models.dedup();
    models
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(role: MsgRole, s: &str) -> ChatTurn {
        ChatTurn { role, content: s.into() }
    }

    #[test]
    fn normalizes_turns() {
        let t = normalize_turns(&[
            turn(MsgRole::Assistant, "hi"),
            turn(MsgRole::User, "a"),
            turn(MsgRole::User, "b"),
            turn(MsgRole::Assistant, "  "),
            turn(MsgRole::User, "c"),
        ]);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].content, "a\n\nb\n\nc");
    }

    #[test]
    fn builds_bodies() {
        let turns = [turn(MsgRole::User, "hello")];
        let p =
            ChatParams { system_prompt: "be cute".into(), temperature: Some(1.5), top_p: None, max_tokens: Some(100) };
        let o = build_request_body(ProviderKind::OpenAiCompatible, "gpt", &turns, &p);
        assert_eq!(o["messages"][0]["role"], "system");
        assert_eq!(o["max_tokens"], 100);
        assert!(o.get("top_p").is_none());
        let a = build_request_body(ProviderKind::Anthropic, "claude", &turns, &p);
        assert_eq!(a["system"], "be cute");
        assert_eq!(a["temperature"], 1.0);
        let g = build_request_body(ProviderKind::Gemini, "gemini", &turns, &p);
        assert_eq!(g["systemInstruction"]["parts"][0]["text"], "be cute");
        assert_eq!(g["generationConfig"]["maxOutputTokens"], 100);
        let none = build_request_body(ProviderKind::Anthropic, "c", &turns, &ChatParams::default());
        assert_eq!(none["max_tokens"], ANTHROPIC_DEFAULT_MAX_TOKENS);
    }

    #[test]
    fn parses_provider_events() {
        use ProviderKind::*;
        assert_eq!(
            parse_event(OpenAiCompatible, None, r#"{"choices":[{"delta":{"content":"Hi"}}]}"#),
            Chunk::Text("Hi".into())
        );
        assert_eq!(parse_event(OpenAiCompatible, None, "[DONE]"), Chunk::Done);
        assert_eq!(
            parse_event(OpenAiCompatible, None, r#"{"choices":[{"delta":{"role":"assistant"}}]}"#),
            Chunk::Ignore
        );
        assert_eq!(
            parse_event(
                Anthropic,
                Some("content_block_delta"),
                r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"Yo"}}"#
            ),
            Chunk::Text("Yo".into())
        );
        assert_eq!(
            parse_event(
                Anthropic,
                Some("error"),
                r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#
            ),
            Chunk::Error("Overloaded".into())
        );
        assert_eq!(parse_event(Anthropic, Some("message_stop"), r#"{"type":"message_stop"}"#), Chunk::Done);
        assert_eq!(
            parse_event(Gemini, None, r#"{"candidates":[{"content":{"parts":[{"text":"A"},{"text":"B"}]}}]}"#),
            Chunk::Text("AB".into())
        );
    }

    #[test]
    fn parses_model_lists() {
        let v: Value = serde_json::from_str(r#"{"data":[{"id":"b"},{"id":"a"}]}"#).unwrap();
        assert_eq!(parse_model_list(ProviderKind::OpenAiCompatible, &v), vec!["a", "b"]);
        let g: Value = serde_json::from_str(
            r#"{"models":[{"name":"models/gemini-x","supportedGenerationMethods":["generateContent"]},
                          {"name":"models/embed","supportedGenerationMethods":["embedContent"]}]}"#,
        )
        .unwrap();
        assert_eq!(parse_model_list(ProviderKind::Gemini, &g), vec!["gemini-x"]);
    }
}
