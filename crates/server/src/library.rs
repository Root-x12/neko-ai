//! The user's own library: memories across chats, a searchable knowledge base of their files
//! (retrieval-augmented generation via SQLite FTS5/BM25, fully offline), and data export.

use axum::{
    extract::{Multipart, Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, put},
    Json, Router,
};
use neko_shared::*;
use serde::Deserialize;

use crate::{
    admin::{audit, workspace_of, ClientIp},
    auth::AuthUser,
    crypto, db,
    error::{AppError, AppResult},
    guard,
    tools::web::truncate,
    uploads, AppState,
};

const MAX_MEMORY_CHARS: usize = 500;
/// Memories included in the prompt are capped so they never crowd out the conversation.
const MEMORY_PROMPT_CHARS: usize = 6_000;
const CHUNK_CHARS: usize = 1_200;
const CHUNK_OVERLAP: usize = 200;
const MAX_CHUNKS: usize = 5_000;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/memory", get(list_memories).post(add_memory).delete(clear_memories))
        .route("/memory/settings", get(get_memory_settings).put(put_memory_settings))
        .route("/memory/{id}", put(update_memory).delete(delete_memory))
        .route(
            "/knowledge",
            get(list_documents)
                .post(upload_document)
                .layer(axum::extract::DefaultBodyLimit::max(uploads::MAX_UPLOAD + 64 * 1024)),
        )
        .route("/knowledge/search", get(search))
        .route("/knowledge/{id}", delete(delete_document))
        .route("/conversations/{id}/export", get(export_conversation))
        .route("/export", get(export_all))
}

// ---------- memory ----------

/// Cleans a memory: trims, collapses whitespace, removes secrets (they'd be sent with every chat).
pub fn clean_memory(text: &str) -> AppResult<String> {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.is_empty() {
        return Err(AppError::bad_request("A memory can't be empty"));
    }
    if flat.chars().count() > MAX_MEMORY_CHARS {
        return Err(AppError::bad_request("Keep memories short (500 characters max)"));
    }
    let secrets: Vec<guard::Match> = guard::scan_sensitive(&flat).into_iter().filter(|m| m.secret).collect();
    Ok(guard::redact(&flat, &secrets))
}

/// The block added to the system prompt when memory is on.
pub fn memory_prompt(memories: &[Memory]) -> Option<String> {
    if memories.is_empty() {
        return None;
    }
    let mut out = String::from(
        "Things you remember about the user from earlier chats (they can view and edit this list; use it when relevant, don't recite it):\n",
    );
    for m in memories.iter().rev() {
        let line = format!("- {}\n", m.content);
        if out.len() + line.len() > MEMORY_PROMPT_CHARS {
            break;
        }
        out.push_str(&line);
    }
    Some(out)
}

async fn list_memories(State(s): State<AppState>, a: AuthUser) -> AppResult<Json<Vec<Memory>>> {
    Ok(Json(s.db.call(move |c| db::list_memories(c, a.user.id)).await?))
}

async fn add_memory(State(s): State<AppState>, a: AuthUser, Json(m): Json<MemoryInput>) -> AppResult<Json<Memory>> {
    let content = clean_memory(&m.content)?;
    s.db.call(move |c| db::add_memory(c, a.user.id, &content, None))
        .await?
        .map(Json)
        .ok_or_else(|| AppError::bad_request(format!("You can keep up to {} memories", db::MAX_MEMORIES)))
}

async fn update_memory(
    State(s): State<AppState>,
    a: AuthUser,
    Path(id): Path<i64>,
    Json(m): Json<MemoryInput>,
) -> AppResult<Json<Memory>> {
    let content = clean_memory(&m.content)?;
    s.db.call(move |c| db::update_memory(c, a.user.id, id, &content))
        .await?
        .map(Json)
        .ok_or_else(|| AppError::not_found("Memory"))
}

async fn delete_memory(State(s): State<AppState>, a: AuthUser, Path(id): Path<i64>) -> AppResult<StatusCode> {
    if s.db.call(move |c| db::delete_memory(c, a.user.id, id)).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::not_found("Memory"))
    }
}

async fn clear_memories(State(s): State<AppState>, a: AuthUser, ip: ClientIp) -> AppResult<StatusCode> {
    let uid = a.user.id;
    let n = s.db.call(move |c| db::clear_memories(c, uid)).await?;
    audit(&s, Some(&a.user), &ip.0, "memory.clear", "", format!("{n} memories")).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_memory_settings(State(s): State<AppState>, a: AuthUser) -> AppResult<Json<MemorySettings>> {
    Ok(Json(s.db.call(move |c| db::memory_settings(c, a.user.id)).await?))
}

async fn put_memory_settings(
    State(s): State<AppState>,
    a: AuthUser,
    Json(m): Json<MemorySettings>,
) -> AppResult<Json<MemorySettings>> {
    s.db.call(move |c| db::save_memory_settings(c, a.user.id, m)).await?;
    Ok(Json(m))
}

// ---------- knowledge ----------

/// Splits text into overlapping chunks along paragraph and sentence boundaries.
pub fn chunk_text(text: &str) -> Vec<String> {
    let mut pieces: Vec<String> = Vec::new();
    for para in text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
        if para.chars().count() <= CHUNK_CHARS {
            pieces.push(para.to_string());
            continue;
        }
        // Long paragraph: break at sentence ends, then hard-split anything still too long.
        let mut cur = String::new();
        for sentence in para.split_inclusive(['.', '!', '?', '\n']) {
            if cur.chars().count() + sentence.chars().count() > CHUNK_CHARS && !cur.is_empty() {
                pieces.push(std::mem::take(&mut cur).trim().to_string());
            }
            cur.push_str(sentence);
            while cur.chars().count() > CHUNK_CHARS {
                let head: String = cur.chars().take(CHUNK_CHARS).collect();
                cur = cur.chars().skip(CHUNK_CHARS).collect();
                pieces.push(head);
            }
        }
        if !cur.trim().is_empty() {
            pieces.push(cur.trim().to_string());
        }
    }
    // Pack pieces into chunks, carrying the tail of each chunk into the next for context.
    let mut chunks: Vec<String> = Vec::new();
    let mut cur = String::new();
    for p in pieces {
        if !cur.is_empty() && cur.chars().count() + p.chars().count() + 2 > CHUNK_CHARS {
            let chars: Vec<char> = cur.chars().collect();
            let tail: String = if chars.len() <= CHUNK_OVERLAP {
                cur.clone()
            } else {
                // Cut mid-chunk: start the overlap at the next word boundary.
                let t: String = chars[chars.len() - CHUNK_OVERLAP..].iter().collect();
                t.split_once(' ').map(|(_, rest)| rest.to_string()).unwrap_or(t)
            };
            chunks.push(std::mem::take(&mut cur));
            cur = tail;
        }
        if !cur.is_empty() {
            cur.push_str("\n\n");
        }
        cur.push_str(&p);
        if chunks.len() >= MAX_CHUNKS {
            break;
        }
    }
    if !cur.trim().is_empty() && chunks.len() < MAX_CHUNKS {
        chunks.push(cur);
    }
    chunks
}

async fn list_documents(State(s): State<AppState>, a: AuthUser) -> AppResult<Json<Vec<Document>>> {
    Ok(Json(s.db.call(move |c| db::list_documents(c, a.user.id)).await?))
}

async fn upload_document(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    mut form: Multipart,
) -> AppResult<Json<Document>> {
    let mut found = None;
    while let Some(field) = form.next_field().await.map_err(|e| AppError::bad_request(format!("Upload failed: {e}")))? {
        if field.name() != Some("file") {
            continue;
        }
        let name = uploads::clean_name(field.file_name().unwrap_or("file"));
        let bytes = field.bytes().await.map_err(|e| AppError::bad_request(format!("Upload failed: {e}")))?;
        found = Some((name, bytes));
        break;
    }
    let (name, bytes) = found.ok_or_else(|| AppError::bad_request("No file in the upload"))?;
    if bytes.is_empty() {
        return Err(AppError::bad_request("The file is empty"));
    }
    let (kind, mime) = uploads::classify(&bytes, &name).map_err(AppError::bad_request)?;
    let text = match kind {
        AttachmentKind::Image => {
            return Err(AppError::bad_request("Images have no text to search. Attach them to a message instead."))
        }
        AttachmentKind::Pdf => uploads::pdf_text(bytes.to_vec()).await.map_err(|e| {
            AppError::bad_request(format!("Couldn't read text from this PDF ({e}). Scanned PDFs need OCR first."))
        })?,
        AttachmentKind::Text => String::from_utf8_lossy(&bytes).into_owned(),
    };
    let chunks = chunk_text(&text);
    if chunks.is_empty() {
        return Err(AppError::bad_request("This file has no text to index"));
    }
    let flags = guard::scan_injection(&text);
    let dir = uploads::uploads_dir(&s.config.data_dir);
    tokio::fs::create_dir_all(&dir).await.map_err(AppError::internal)?;
    let file_name = format!("doc-{}.bin", crypto::random_token());
    tokio::fs::write(dir.join(&file_name), &bytes).await.map_err(AppError::internal)?;
    let (uid, size, stored, n) = (a.user.id, bytes.len() as i64, file_name.clone(), name.clone());
    match s.db.call(move |c| db::insert_document(c, uid, &n, &mime, size, &stored, &flags, &chunks)).await {
        Ok(doc) => {
            audit(&s, Some(&a.user), &ip.0, "knowledge.add", &doc.name, format!("{} chunks", doc.chunks)).await;
            Ok(Json(doc))
        }
        Err(e) => {
            let _ = tokio::fs::remove_file(dir.join(&file_name)).await;
            Err(e.into())
        }
    }
}

async fn delete_document(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let uid = a.user.id;
    let path =
        s.db.call(move |c| db::delete_document(c, uid, id)).await?.ok_or_else(|| AppError::not_found("Document"))?;
    uploads::remove_files(&s.config.data_dir, vec![path]).await;
    audit(&s, Some(&a.user), &ip.0, "knowledge.delete", id.to_string(), "").await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

async fn search(
    State(s): State<AppState>,
    a: AuthUser,
    Query(q): Query<SearchQuery>,
) -> AppResult<Json<Vec<KnowledgeHit>>> {
    let q: String = q.q.chars().take(300).collect();
    Ok(Json(s.db.call(move |c| db::search_knowledge(c, a.user.id, &q, 8)).await?))
}

/// Text for the `search_knowledge` tool. Retrieved text is about to leave the server, so secrets
/// (and personal data, if the workspace redacts it) are replaced first.
pub async fn knowledge_tool(s: &AppState, user: &User, query: &str) -> Result<String, String> {
    let (uid, q) = (user.id, query.to_string());
    let hits = s.db.call(move |c| db::search_knowledge(c, uid, &q, 6)).await.map_err(|e| e.to_string())?;
    if hits.is_empty() {
        return Ok(format!("No passages in the user's files match “{query}”. Try other keywords."));
    }
    let ws = workspace_of(s, user).await;
    let redact_pii = matches!(ws.pii_policy, GuardPolicy::Redact | GuardPolicy::Block);
    let redact_secrets = ws.secret_policy != GuardPolicy::Off;
    let mut out = format!("Passages from the user's files matching “{query}” (cite the file names):\n");
    for h in hits {
        let found: Vec<guard::Match> = guard::scan_sensitive(&h.text)
            .into_iter()
            .filter(|m| (m.secret && redact_secrets) || (!m.secret && redact_pii))
            .collect();
        let text = guard::redact(&h.text, &found);
        out.push_str(&format!("\n--- {} (part {}) ---\n{}\n", h.document, h.chunk + 1, truncate(text.trim(), 2_000)));
    }
    Ok(out)
}

// ---------- export ----------

#[derive(Deserialize)]
struct ExportQuery {
    #[serde(default)]
    format: Option<String>,
}

fn date(ts: i64) -> String {
    // Days since 1970 → civil date (Howard Hinnant's algorithm), to avoid a date-time dependency.
    let days = ts.div_euclid(86_400);
    let secs = ts.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", secs / 3600, secs % 3600 / 60)
}

pub fn conversation_markdown(conv: &Conversation) -> String {
    let mut md = format!("# {}\n\n", conv.title);
    md.push_str(&format!(
        "_Exported from Neko AI · started {} · last active {}_\n\n",
        date(conv.created_at),
        date(conv.updated_at)
    ));
    if !conv.params.system_prompt.trim().is_empty() {
        md.push_str(&format!("> **System prompt:** {}\n\n", conv.params.system_prompt.trim().replace('\n', "\n> ")));
    }
    for m in &conv.messages {
        match m.role {
            MsgRole::User => md.push_str(&format!("---\n\n### 🧑 You · {}\n\n", date(m.created_at))),
            MsgRole::Assistant => md.push_str(&format!(
                "---\n\n### 🐱 {} · {}\n\n",
                m.model.as_deref().unwrap_or("Assistant"),
                date(m.created_at)
            )),
        }
        for a in &m.attachments {
            md.push_str(&format!("📎 *{}* ({} bytes)\n\n", a.name, a.size));
        }
        for t in &m.tool_calls {
            if t.name.is_empty() {
                continue;
            }
            md.push_str(&format!(
                "> 🔧 **{}** `{}`{}\n\n",
                t.label,
                truncate(&t.arguments, 200),
                if t.is_error { " — failed" } else { "" }
            ));
        }
        md.push_str(m.content.trim());
        md.push_str("\n\n");
        if let Some(e) = &m.error {
            md.push_str(&format!("*({e})*\n\n"));
        }
    }
    md
}

fn file_name(title: &str, ext: &str) -> String {
    let base: String = title
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .take(8)
        .collect::<Vec<_>>()
        .join("-");
    format!("{}.{ext}", if base.is_empty() { "chat".into() } else { base })
}

fn download(body: String, mime: &str, name: &str) -> Response {
    (
        [
            (header::CONTENT_TYPE, mime.to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        body,
    )
        .into_response()
}

async fn export_conversation(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    Path(id): Path<i64>,
    Query(q): Query<ExportQuery>,
) -> AppResult<Response> {
    let uid = a.user.id;
    let conv =
        s.db.call(move |c| db::get_conversation(c, uid, id))
            .await?
            .ok_or_else(|| AppError::not_found("Conversation"))?;
    let format = q.format.unwrap_or_else(|| "markdown".into());
    audit(&s, Some(&a.user), &ip.0, "chat.export", id.to_string(), &format).await;
    match format.as_str() {
        "markdown" | "md" => {
            Ok(download(conversation_markdown(&conv), "text/markdown; charset=utf-8", &file_name(&conv.title, "md")))
        }
        "json" => {
            let body = serde_json::to_string_pretty(&conv).map_err(AppError::internal)?;
            Ok(download(body, "application/json", &file_name(&conv.title, "json")))
        }
        _ => Err(AppError::bad_request("Format must be markdown or json (PDF is made in the browser)")),
    }
}

async fn export_all(State(s): State<AppState>, a: AuthUser, ip: ClientIp) -> AppResult<Response> {
    let uid = a.user.id;
    let (conversations, memories, documents) = s
        .db
        .call(move |c| Ok((db::all_conversations(c, uid)?, db::list_memories(c, uid)?, db::list_documents(c, uid)?)))
        .await?;
    let n = conversations.len();
    let export = DataExport {
        format: "neko-ai-export/1".into(),
        exported_at: db::now(),
        user: a.user.clone(),
        conversations,
        memories,
        documents,
    };
    audit(&s, Some(&a.user), &ip.0, "data.export", "", format!("{n} chats")).await;
    let body = serde_json::to_string_pretty(&export).map_err(AppError::internal)?;
    Ok(download(body, "application/json", "neko-ai-export.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_with_overlap() {
        let para = "Cats are great. ".repeat(200); // ~3200 chars, one paragraph
        let text = format!("Intro line.\n\n{para}\n\nOutro line.");
        let chunks = chunk_text(&text);
        assert!(chunks.len() >= 3, "{}", chunks.len());
        assert!(chunks.iter().all(|c| c.chars().count() <= CHUNK_CHARS + CHUNK_OVERLAP + 2));
        assert!(chunks[0].starts_with("Intro line."));
        assert!(chunks.last().unwrap().ends_with("Outro line."));
        // Consecutive chunks share text.
        let tail: String = chunks[0].chars().rev().take(40).collect::<Vec<_>>().into_iter().rev().collect();
        assert!(chunks[1].contains(tail.trim()), "overlap missing");
        assert!(chunk_text("   \n\n  ").is_empty());
        let huge = "x".repeat(5000);
        assert!(chunk_text(&huge).iter().all(|c| c.chars().count() <= CHUNK_CHARS + CHUNK_OVERLAP + 2));
    }

    #[test]
    fn memories_drop_secrets() {
        assert_eq!(clean_memory("  likes   cats ").unwrap(), "likes cats");
        assert!(clean_memory("key sk-proj-abcdefghijklmnopqrstuvwx12").unwrap().contains("[REDACTED"));
        assert!(clean_memory("").is_err());
        assert!(clean_memory(&"a".repeat(600)).is_err());
        let p =
            memory_prompt(&[Memory { id: 1, content: "Prefers Rust".into(), created_at: 0, updated_at: 0 }]).unwrap();
        assert!(p.contains("- Prefers Rust"));
        assert!(memory_prompt(&[]).is_none());
    }

    #[test]
    fn exports_markdown() {
        let conv = Conversation {
            id: 1,
            title: "Cats & Code!".into(),
            provider_id: None,
            model: None,
            params: ChatParams { system_prompt: "Be cute".into(), ..Default::default() },
            messages: vec![
                Message {
                    id: 1,
                    role: MsgRole::User,
                    content: "hi".into(),
                    provider_id: None,
                    model: None,
                    error: None,
                    created_at: 0,
                    attachments: vec![],
                    tool_calls: vec![],
                },
                Message {
                    id: 2,
                    role: MsgRole::Assistant,
                    content: "**meow**".into(),
                    provider_id: None,
                    model: Some("m1".into()),
                    error: None,
                    created_at: 86_400 * 365,
                    attachments: vec![],
                    tool_calls: vec![],
                },
            ],
            created_at: 0,
            updated_at: 1_762_178_400,
        };
        let md = conversation_markdown(&conv);
        assert!(md.starts_with("# Cats & Code!"));
        assert!(md.contains("> **System prompt:** Be cute"));
        assert!(md.contains("### 🐱 m1 · 1971-01-01 00:00 UTC"));
        assert!(md.contains("**meow**"));
        assert_eq!(file_name("Cats & Code!", "md"), "cats-code.md");
        assert_eq!(date(1_762_178_400), "2025-11-03 14:00 UTC");
    }
}
