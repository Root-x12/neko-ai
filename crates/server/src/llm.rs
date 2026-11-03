//! Provider adapters. Every provider is turned into one stream of [`LlmEvent`]s (text deltas and
//! complete tool calls), and our provider-neutral [`Turn`]s are encoded into each provider's format.

use std::{collections::BTreeMap, pin::Pin, time::Duration};

use futures_util::{Stream, StreamExt};
use neko_shared::{sse::SseParser, AttachmentKind, ChatParams, ProviderKind};
use serde_json::{json, Value};

pub type EventStream = Pin<Box<dyn Stream<Item = Result<LlmEvent, String>> + Send>>;

/// Everything needed to call a provider (the key is already decrypted).
#[derive(Clone)]
pub struct Endpoint {
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key: Option<String>,
}

/// A file sent along with a user turn.
#[derive(Debug, Clone)]
pub struct Media {
    pub kind: AttachmentKind,
    pub name: String,
    pub mime: String,
    /// Base64 bytes (images and PDFs).
    pub b64: String,
    /// Extracted text (PDFs and text files). Used where the provider can't read the file natively.
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    /// Gemini "thought signature" that must be echoed back with the call.
    pub signature: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CallResult {
    pub id: String,
    pub name: String,
    pub content: String,
    pub is_error: bool,
}

#[derive(Debug, Clone)]
pub enum Turn {
    User { text: String, media: Vec<Media> },
    Assistant { text: String, calls: Vec<Call> },
    ToolResults(Vec<CallResult>),
}

impl Turn {
    pub fn user(text: impl Into<String>) -> Self {
        Turn::User { text: text.into(), media: Vec::new() }
    }
    pub fn assistant(text: impl Into<String>) -> Self {
        Turn::Assistant { text: text.into(), calls: Vec::new() }
    }
}

#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    /// JSON Schema for the arguments (an object schema).
    pub schema: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LlmEvent {
    Text(String),
    Call(Call),
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

/// Cleans up a conversation so every provider accepts it: drops empty turns, merges consecutive
/// user/assistant turns, and makes sure it starts with the user.
pub fn normalize_turns(turns: &[Turn]) -> Vec<Turn> {
    let mut out: Vec<Turn> = Vec::new();
    for t in turns {
        match t {
            Turn::User { text, media } if text.trim().is_empty() && media.is_empty() => continue,
            Turn::Assistant { text, calls } if text.trim().is_empty() && calls.is_empty() => continue,
            Turn::ToolResults(r) if r.is_empty() => continue,
            _ => {}
        }
        match (out.last_mut(), t) {
            (Some(Turn::User { text: a, media: am }), Turn::User { text: b, media: bm }) => {
                join_text(a, b);
                am.extend(bm.iter().cloned());
            }
            (Some(Turn::Assistant { text: a, calls: ac }), Turn::Assistant { text: b, calls: bc }) if ac.is_empty() => {
                join_text(a, b);
                ac.extend(bc.iter().cloned());
            }
            _ => out.push(t.clone()),
        }
    }
    while matches!(out.first(), Some(Turn::Assistant { .. } | Turn::ToolResults(_))) {
        out.remove(0);
    }
    out
}

fn join_text(a: &mut String, b: &str) {
    if b.trim().is_empty() {
        return;
    }
    if !a.trim().is_empty() {
        a.push_str("\n\n");
    }
    a.push_str(b);
}

fn file_as_text(m: &Media) -> String {
    let body = m.text.as_deref().unwrap_or("(no text could be extracted)");
    format!("[Attached file: {}]\n{}", m.name, body)
}

/// Appends a message, merging it into the previous one if the role repeats.
fn push_merged(messages: &mut Vec<Value>, role: &str, parts_key: &str, mut parts: Vec<Value>) {
    if let Some(last) = messages.last_mut() {
        if last["role"] == role {
            if let Some(arr) = last[parts_key].as_array_mut() {
                arr.append(&mut parts);
                return;
            }
        }
    }
    messages.push(json!({ "role": role, parts_key: parts }));
}

pub fn build_request_body(kind: ProviderKind, model: &str, turns: &[Turn], p: &ChatParams, tools: &[ToolDef]) -> Value {
    let system = p.system_prompt.trim();
    match kind {
        ProviderKind::OpenAiCompatible => {
            let mut messages = Vec::new();
            if !system.is_empty() {
                messages.push(json!({ "role": "system", "content": system }));
            }
            for t in turns {
                match t {
                    Turn::User { text, media } if media.is_empty() => {
                        messages.push(json!({ "role": "user", "content": text }));
                    }
                    Turn::User { text, media } => {
                        let mut parts = Vec::new();
                        for m in media {
                            match m.kind {
                                AttachmentKind::Image => parts.push(json!({
                                    "type": "image_url",
                                    "image_url": { "url": format!("data:{};base64,{}", m.mime, m.b64) }
                                })),
                                _ => parts.push(json!({ "type": "text", "text": file_as_text(m) })),
                            }
                        }
                        if !text.trim().is_empty() {
                            parts.push(json!({ "type": "text", "text": text }));
                        }
                        messages.push(json!({ "role": "user", "content": parts }));
                    }
                    Turn::Assistant { text, calls } => {
                        let mut m = json!({ "role": "assistant", "content": if text.is_empty() { Value::Null } else { json!(text) } });
                        if !calls.is_empty() {
                            m["tool_calls"] = calls
                                .iter()
                                .map(|c| json!({ "id": c.id, "type": "function", "function": { "name": c.name, "arguments": c.arguments.to_string() } }))
                                .collect();
                        }
                        messages.push(m);
                    }
                    Turn::ToolResults(results) => {
                        for r in results {
                            messages.push(json!({ "role": "tool", "tool_call_id": r.id, "content": r.content }));
                        }
                    }
                }
            }
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
            if !tools.is_empty() {
                body["tools"] = tools
                    .iter()
                    .map(|t| json!({ "type": "function", "function": { "name": t.name, "description": t.description, "parameters": t.schema } }))
                    .collect();
            }
            body
        }
        ProviderKind::Anthropic => {
            let mut messages: Vec<Value> = Vec::new();
            for t in turns {
                match t {
                    Turn::User { text, media } => {
                        let mut blocks = Vec::new();
                        for m in media {
                            blocks.push(match m.kind {
                                AttachmentKind::Image => json!({
                                    "type": "image",
                                    "source": { "type": "base64", "media_type": m.mime, "data": m.b64 }
                                }),
                                AttachmentKind::Pdf => json!({
                                    "type": "document",
                                    "source": { "type": "base64", "media_type": "application/pdf", "data": m.b64 },
                                    "title": m.name
                                }),
                                AttachmentKind::Text => json!({ "type": "text", "text": file_as_text(m) }),
                            });
                        }
                        if !text.trim().is_empty() {
                            blocks.push(json!({ "type": "text", "text": text }));
                        }
                        push_merged(&mut messages, "user", "content", blocks);
                    }
                    Turn::Assistant { text, calls } => {
                        let mut blocks = Vec::new();
                        if !text.trim().is_empty() {
                            blocks.push(json!({ "type": "text", "text": text }));
                        }
                        for c in calls {
                            blocks
                                .push(json!({ "type": "tool_use", "id": c.id, "name": c.name, "input": c.arguments }));
                        }
                        push_merged(&mut messages, "assistant", "content", blocks);
                    }
                    Turn::ToolResults(results) => {
                        let blocks = results
                            .iter()
                            .map(|r| json!({ "type": "tool_result", "tool_use_id": r.id, "content": r.content, "is_error": r.is_error }))
                            .collect();
                        push_merged(&mut messages, "user", "content", blocks);
                    }
                }
            }
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
            if !tools.is_empty() {
                body["tools"] = tools
                    .iter()
                    .map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.schema }))
                    .collect();
            }
            body
        }
        ProviderKind::Gemini => {
            let mut contents: Vec<Value> = Vec::new();
            for t in turns {
                match t {
                    Turn::User { text, media } => {
                        let mut parts = Vec::new();
                        for m in media {
                            parts.push(match m.kind {
                                AttachmentKind::Image | AttachmentKind::Pdf => {
                                    json!({ "inline_data": { "mime_type": m.mime, "data": m.b64 } })
                                }
                                AttachmentKind::Text => json!({ "text": file_as_text(m) }),
                            });
                        }
                        if !text.trim().is_empty() {
                            parts.push(json!({ "text": text }));
                        }
                        push_merged(&mut contents, "user", "parts", parts);
                    }
                    Turn::Assistant { text, calls } => {
                        let mut parts = Vec::new();
                        if !text.trim().is_empty() {
                            parts.push(json!({ "text": text }));
                        }
                        for c in calls {
                            let mut part = json!({ "functionCall": { "name": c.name, "args": c.arguments } });
                            if let Some(sig) = &c.signature {
                                part["thoughtSignature"] = json!(sig);
                            }
                            parts.push(part);
                        }
                        push_merged(&mut contents, "model", "parts", parts);
                    }
                    Turn::ToolResults(results) => {
                        let parts = results
                            .iter()
                            .map(|r| {
                                let key = if r.is_error { "error" } else { "content" };
                                json!({ "functionResponse": { "name": r.name, "response": { key: r.content } } })
                            })
                            .collect();
                        push_merged(&mut contents, "user", "parts", parts);
                    }
                }
            }
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
            if !tools.is_empty() {
                let decls: Vec<Value> = tools
                    .iter()
                    .map(|t| json!({ "name": t.name, "description": t.description, "parametersJsonSchema": t.schema }))
                    .collect();
                body["tools"] = json!([{ "functionDeclarations": decls }]);
            }
            body
        }
    }
}

/// What one provider SSE event produced.
#[derive(Debug, PartialEq)]
pub enum Out {
    Event(LlmEvent),
    Error(String),
    Done,
}

#[derive(Default, Debug)]
struct PartialCall {
    id: String,
    name: String,
    args: String,
}

impl PartialCall {
    fn finish(self) -> Call {
        let arguments = if self.args.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&self.args).unwrap_or_else(|_| json!({ "_raw": self.args }))
        };
        let id = if self.id.is_empty() { new_call_id() } else { sanitize_id(&self.id) };
        Call { id, name: self.name, arguments, signature: None }
    }
}

pub fn new_call_id() -> String {
    format!("call_{}", &crate::crypto::random_token()[..16])
}

/// Tool-call ids must be `[A-Za-z0-9_-]` for Anthropic; keep provider ids when they comply.
fn sanitize_id(id: &str) -> String {
    let clean: String = id.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')).take(64).collect();
    if clean.is_empty() {
        new_call_id()
    } else {
        clean
    }
}

/// Stateful decoder for one provider stream: tool-call arguments arrive in pieces.
pub struct Decoder {
    kind: ProviderKind,
    partial: BTreeMap<u64, PartialCall>,
}

impl Decoder {
    pub fn new(kind: ProviderKind) -> Self {
        Self { kind, partial: BTreeMap::new() }
    }

    pub fn feed(&mut self, event: Option<&str>, data: &str) -> Vec<Out> {
        if data.trim() == "[DONE]" {
            let mut out = self.flush();
            out.push(Out::Done);
            return out;
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else { return Vec::new() };
        if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
            return vec![Out::Error(error_text(err))];
        }
        let mut out = Vec::new();
        match self.kind {
            ProviderKind::OpenAiCompatible => {
                let choice = &v["choices"][0];
                if let Some(text) = choice["delta"]["content"].as_str().filter(|t| !t.is_empty()) {
                    out.push(Out::Event(LlmEvent::Text(text.to_string())));
                }
                for tc in choice["delta"]["tool_calls"].as_array().into_iter().flatten() {
                    let idx = tc["index"].as_u64().unwrap_or(self.partial.len() as u64);
                    let p = self.partial.entry(idx).or_default();
                    if let Some(id) = tc["id"].as_str() {
                        p.id = id.to_string();
                    }
                    if let Some(name) = tc["function"]["name"].as_str() {
                        p.name.push_str(name);
                    }
                    if let Some(args) = tc["function"]["arguments"].as_str() {
                        p.args.push_str(args);
                    }
                }
                if choice["finish_reason"].is_string() {
                    out.extend(self.flush());
                }
            }
            ProviderKind::Anthropic => match event.or(v["type"].as_str()) {
                Some("content_block_start") if v["content_block"]["type"] == "tool_use" => {
                    let idx = v["index"].as_u64().unwrap_or(0);
                    self.partial.insert(
                        idx,
                        PartialCall {
                            id: v["content_block"]["id"].as_str().unwrap_or_default().to_string(),
                            name: v["content_block"]["name"].as_str().unwrap_or_default().to_string(),
                            args: String::new(),
                        },
                    );
                }
                Some("content_block_delta") => {
                    let idx = v["index"].as_u64().unwrap_or(0);
                    match v["delta"]["type"].as_str() {
                        Some("text_delta") => {
                            let t = v["delta"]["text"].as_str().unwrap_or_default();
                            if !t.is_empty() {
                                out.push(Out::Event(LlmEvent::Text(t.to_string())));
                            }
                        }
                        Some("input_json_delta") => {
                            if let Some(p) = self.partial.get_mut(&idx) {
                                p.args.push_str(v["delta"]["partial_json"].as_str().unwrap_or_default());
                            }
                        }
                        _ => {}
                    }
                }
                Some("content_block_stop") => {
                    if let Some(p) = self.partial.remove(&v["index"].as_u64().unwrap_or(0)) {
                        out.push(Out::Event(LlmEvent::Call(p.finish())));
                    }
                }
                Some("message_stop") => {
                    out.extend(self.flush());
                    out.push(Out::Done);
                }
                Some("error") => out.push(Out::Error(error_text(&v))),
                _ => {}
            },
            ProviderKind::Gemini => {
                for part in v["candidates"][0]["content"]["parts"].as_array().into_iter().flatten() {
                    if let Some(text) = part["text"].as_str().filter(|t| !t.is_empty()) {
                        if part["thought"] != true {
                            out.push(Out::Event(LlmEvent::Text(text.to_string())));
                        }
                    }
                    if let Some(fc) = part.get("functionCall") {
                        out.push(Out::Event(LlmEvent::Call(Call {
                            id: new_call_id(),
                            name: fc["name"].as_str().unwrap_or_default().to_string(),
                            arguments: fc.get("args").cloned().unwrap_or_else(|| json!({})),
                            signature: part["thoughtSignature"].as_str().map(str::to_string),
                        })));
                    }
                }
                if out.is_empty() {
                    if let Some(reason) = v["promptFeedback"]["blockReason"].as_str() {
                        out.push(Out::Error(format!("Gemini blocked the prompt ({reason})")));
                    }
                }
            }
        }
        out
    }

    /// Emits tool calls still being assembled (stream ended without an explicit stop).
    pub fn flush(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.partial).into_values().map(|p| Out::Event(LlmEvent::Call(p.finish()))).collect()
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
    turns: &[Turn],
    params: &ChatParams,
    tools: &[ToolDef],
) -> Result<EventStream, String> {
    let turns = normalize_turns(turns);
    if turns.is_empty() {
        return Err("Nothing to send".into());
    }
    let body = build_request_body(ep.kind, model, &turns, params, tools);
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
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<LlmEvent, String>>(64);
    tokio::spawn(async move {
        let mut parser = SseParser::new();
        let mut decoder = Decoder::new(kind);
        // Returns false when the stream is over (done, error, or nobody listening).
        async fn forward(tx: &tokio::sync::mpsc::Sender<Result<LlmEvent, String>>, outs: Vec<Out>) -> bool {
            for o in outs {
                let keep_going = match o {
                    Out::Event(e) => tx.send(Ok(e)).await.is_ok(),
                    Out::Error(e) => {
                        let _ = tx.send(Err(e)).await;
                        false
                    }
                    Out::Done => false,
                };
                if !keep_going {
                    return false;
                }
            }
            true
        }
        while let Some(chunk) = bytes.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(Err(format!("Stream interrupted: {e}"))).await;
                    return;
                }
            };
            for ev in parser.push(&chunk) {
                let outs = decoder.feed(ev.event.as_deref(), &ev.data);
                if !forward(&tx, outs).await {
                    return;
                }
            }
        }
        let mut outs = parser.finish().map(|ev| decoder.feed(ev.event.as_deref(), &ev.data)).unwrap_or_default();
        outs.extend(decoder.flush());
        forward(&tx, outs).await;
    });
    Ok(Box::pin(futures_util::stream::poll_fn(move |cx| rx.poll_recv(cx))))
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

    fn image() -> Media {
        Media {
            kind: AttachmentKind::Image,
            name: "cat.png".into(),
            mime: "image/png".into(),
            b64: "AAAA".into(),
            text: None,
        }
    }

    fn pdf() -> Media {
        Media {
            kind: AttachmentKind::Pdf,
            name: "doc.pdf".into(),
            mime: "application/pdf".into(),
            b64: "JVBE".into(),
            text: Some("pdf words".into()),
        }
    }

    fn call() -> Call {
        Call {
            id: "call_1".into(),
            name: "web_search".into(),
            arguments: json!({"query": "cats"}),
            signature: Some("sig".into()),
        }
    }

    fn tool_round() -> Vec<Turn> {
        vec![
            Turn::User { text: "look".into(), media: vec![image(), pdf()] },
            Turn::Assistant { text: "Searching".into(), calls: vec![call()] },
            Turn::ToolResults(vec![CallResult {
                id: "call_1".into(),
                name: "web_search".into(),
                content: "found".into(),
                is_error: false,
            }]),
        ]
    }

    fn tools() -> Vec<ToolDef> {
        vec![ToolDef { name: "web_search".into(), description: "Search".into(), schema: json!({"type":"object"}) }]
    }

    #[test]
    fn normalizes_turns() {
        let t = normalize_turns(&[
            Turn::assistant("hi"),
            Turn::user("a"),
            Turn::user("b"),
            Turn::assistant("  "),
            Turn::user("c"),
        ]);
        assert_eq!(t.len(), 1);
        assert!(matches!(&t[0], Turn::User { text, .. } if text == "a\n\nb\n\nc"));
    }

    #[test]
    fn builds_bodies() {
        let turns = [Turn::user("hello")];
        let p =
            ChatParams { system_prompt: "be cute".into(), temperature: Some(1.5), top_p: None, max_tokens: Some(100) };
        let o = build_request_body(ProviderKind::OpenAiCompatible, "gpt", &turns, &p, &[]);
        assert_eq!(o["messages"][0]["role"], "system");
        assert_eq!(o["max_tokens"], 100);
        assert!(o.get("top_p").is_none() && o.get("tools").is_none());
        let a = build_request_body(ProviderKind::Anthropic, "claude", &turns, &p, &[]);
        assert_eq!(a["system"], "be cute");
        assert_eq!(a["temperature"], 1.0);
        let g = build_request_body(ProviderKind::Gemini, "gemini", &turns, &p, &[]);
        assert_eq!(g["systemInstruction"]["parts"][0]["text"], "be cute");
        assert_eq!(g["generationConfig"]["maxOutputTokens"], 100);
        let none = build_request_body(ProviderKind::Anthropic, "c", &turns, &ChatParams::default(), &[]);
        assert_eq!(none["max_tokens"], ANTHROPIC_DEFAULT_MAX_TOKENS);
    }

    #[test]
    fn encodes_tools_and_media_openai() {
        let b =
            build_request_body(ProviderKind::OpenAiCompatible, "m", &tool_round(), &ChatParams::default(), &tools());
        let m = &b["messages"];
        assert_eq!(m[0]["content"][0]["type"], "image_url");
        assert!(m[0]["content"][0]["image_url"]["url"].as_str().unwrap().starts_with("data:image/png;base64,"));
        assert!(m[0]["content"][1]["text"].as_str().unwrap().contains("pdf words"));
        assert_eq!(m[1]["tool_calls"][0]["function"]["arguments"], "{\"query\":\"cats\"}");
        assert_eq!(m[2]["role"], "tool");
        assert_eq!(m[2]["tool_call_id"], "call_1");
        assert_eq!(b["tools"][0]["function"]["name"], "web_search");
    }

    #[test]
    fn encodes_tools_and_media_anthropic() {
        let mut turns = tool_round();
        turns.push(Turn::user("thanks"));
        let b = build_request_body(ProviderKind::Anthropic, "m", &turns, &ChatParams::default(), &tools());
        let m = b["messages"].as_array().unwrap();
        assert_eq!(m.len(), 3, "tool results and the next user text share one user message");
        assert_eq!(m[0]["content"][0]["type"], "image");
        assert_eq!(m[0]["content"][1]["type"], "document");
        assert_eq!(m[1]["content"][1]["type"], "tool_use");
        assert_eq!(m[1]["content"][1]["input"]["query"], "cats");
        assert_eq!(m[2]["content"][0]["type"], "tool_result");
        assert_eq!(m[2]["content"][1]["text"], "thanks");
        assert_eq!(b["tools"][0]["input_schema"]["type"], "object");
    }

    #[test]
    fn encodes_tools_and_media_gemini() {
        let b = build_request_body(ProviderKind::Gemini, "m", &tool_round(), &ChatParams::default(), &tools());
        let c = &b["contents"];
        assert_eq!(c[0]["parts"][0]["inline_data"]["mime_type"], "image/png");
        assert_eq!(c[0]["parts"][1]["inline_data"]["mime_type"], "application/pdf");
        assert_eq!(c[1]["role"], "model");
        assert_eq!(c[1]["parts"][1]["functionCall"]["name"], "web_search");
        assert_eq!(c[1]["parts"][1]["thoughtSignature"], "sig");
        assert_eq!(c[2]["parts"][0]["functionResponse"]["response"]["content"], "found");
        assert_eq!(b["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"]["type"], "object");
    }

    fn events(kind: ProviderKind, items: &[(Option<&str>, &str)]) -> Vec<Out> {
        let mut d = Decoder::new(kind);
        let mut out: Vec<Out> = items.iter().flat_map(|(e, data)| d.feed(*e, data)).collect();
        out.extend(d.flush());
        out
    }

    #[test]
    fn decodes_text_and_errors() {
        use ProviderKind::*;
        assert_eq!(
            events(OpenAiCompatible, &[(None, r#"{"choices":[{"delta":{"content":"Hi"}}]}"#), (None, "[DONE]")]),
            vec![Out::Event(LlmEvent::Text("Hi".into())), Out::Done]
        );
        assert_eq!(
            events(
                Anthropic,
                &[(Some("error"), r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#)]
            ),
            vec![Out::Error("Overloaded".into())]
        );
        assert_eq!(
            events(Gemini, &[(None, r#"{"candidates":[{"content":{"parts":[{"text":"A"},{"text":"B"}]}}]}"#)]),
            vec![Out::Event(LlmEvent::Text("A".into())), Out::Event(LlmEvent::Text("B".into()))]
        );
    }

    #[test]
    fn decodes_openai_tool_calls_in_pieces() {
        let out = events(
            ProviderKind::OpenAiCompatible,
            &[
                (
                    None,
                    r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_abc","function":{"name":"web_search","arguments":"{\"qu"}}]}}]}"#,
                ),
                (
                    None,
                    r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"ery\":\"cats\"}"}}]}}]}"#,
                ),
                (None, r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#),
                (None, "[DONE]"),
            ],
        );
        let Out::Event(LlmEvent::Call(c)) = &out[0] else { panic!("{out:?}") };
        assert_eq!(
            (c.id.as_str(), c.name.as_str(), &c.arguments),
            ("call_abc", "web_search", &json!({"query":"cats"}))
        );
        assert_eq!(out[1], Out::Done);
    }

    #[test]
    fn decodes_anthropic_tool_use() {
        let out = events(
            ProviderKind::Anthropic,
            &[
                (
                    Some("content_block_start"),
                    r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"fetch_url"}}"#,
                ),
                (
                    Some("content_block_delta"),
                    r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"url\":"}}"#,
                ),
                (
                    Some("content_block_delta"),
                    r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"https://x.dev\"}"}}"#,
                ),
                (Some("content_block_stop"), r#"{"type":"content_block_stop","index":1}"#),
                (Some("message_stop"), r#"{"type":"message_stop"}"#),
            ],
        );
        let Out::Event(LlmEvent::Call(c)) = &out[0] else { panic!("{out:?}") };
        assert_eq!(c.id, "toolu_1");
        assert_eq!(c.arguments["url"], "https://x.dev");
    }

    #[test]
    fn decodes_gemini_function_call_with_signature() {
        let out = events(
            ProviderKind::Gemini,
            &[(
                None,
                r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"web_search","args":{"query":"q"}},"thoughtSignature":"abc"}]}}]}"#,
            )],
        );
        let Out::Event(LlmEvent::Call(c)) = &out[0] else { panic!("{out:?}") };
        assert_eq!(c.signature.as_deref(), Some("abc"));
        assert!(c.id.starts_with("call_"));
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
