//! SQLite storage. All queries are synchronous and run on the blocking pool via [`Db::call`].

use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use neko_shared::{
    Attachment, AttachmentKind, ChatParams, Conversation, ConversationSummary, DeviceSession, McpServer, McpTransport,
    Message, MsgRole, Provider, ProviderKind, Role, SearchBackend, SearchHit, SearchSettings, ToolCall, User,
};
use rusqlite::{params, Connection, OptionalExtension, Row};

pub type DbResult<T> = rusqlite::Result<T>;

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

const MIGRATIONS: &[&str] = &[
    r#"
CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    role          TEXT NOT NULL,
    created_at    INTEGER NOT NULL
);
CREATE TABLE sessions (
    id         INTEGER PRIMARY KEY,
    token_hash TEXT NOT NULL UNIQUE,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_agent TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_seen  INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE TABLE providers (
    id          INTEGER PRIMARY KEY,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL,
    base_url    TEXT NOT NULL,
    api_key_enc BLOB,
    key_hint    TEXT NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL
);
CREATE TABLE conversations (
    id            INTEGER PRIMARY KEY,
    user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title         TEXT NOT NULL,
    provider_id   INTEGER REFERENCES providers(id) ON DELETE SET NULL,
    model         TEXT,
    system_prompt TEXT NOT NULL DEFAULT '',
    temperature   REAL,
    top_p         REAL,
    max_tokens    INTEGER,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);
CREATE INDEX conversations_user ON conversations(user_id, updated_at DESC);
CREATE TABLE messages (
    id              INTEGER PRIMARY KEY,
    conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    role            TEXT NOT NULL,
    content         TEXT NOT NULL,
    provider_id     INTEGER,
    model           TEXT,
    error           TEXT,
    created_at      INTEGER NOT NULL
);
CREATE INDEX messages_conversation ON messages(conversation_id, id);
CREATE VIRTUAL TABLE messages_fts USING fts5(content, content='messages', content_rowid='id', tokenize='unicode61');
CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
END;
CREATE TRIGGER messages_ad AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.id, old.content);
END;
CREATE TRIGGER messages_au AFTER UPDATE OF content ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.id, old.content);
    INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
END;
"#,
    r#"
ALTER TABLE messages ADD COLUMN tool_calls TEXT NOT NULL DEFAULT '[]';
CREATE TABLE attachments (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    message_id INTEGER REFERENCES messages(id) ON DELETE SET NULL,
    name       TEXT NOT NULL,
    mime       TEXT NOT NULL,
    kind       TEXT NOT NULL,
    size       INTEGER NOT NULL,
    path       TEXT NOT NULL,
    text       TEXT,
    created_at INTEGER NOT NULL
);
CREATE INDEX attachments_message ON attachments(message_id);
CREATE TABLE search_settings (
    user_id     INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    backend     TEXT NOT NULL,
    base_url    TEXT NOT NULL DEFAULT '',
    api_key_enc BLOB,
    key_hint    TEXT NOT NULL DEFAULT ''
);
CREATE TABLE mcp_servers (
    id           INTEGER PRIMARY KEY,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    transport    TEXT NOT NULL,
    url          TEXT NOT NULL DEFAULT '',
    command      TEXT NOT NULL DEFAULT '',
    args         TEXT NOT NULL DEFAULT '[]',
    headers_enc  BLOB,
    env_enc      BLOB,
    header_names TEXT NOT NULL DEFAULT '[]',
    env_names    TEXT NOT NULL DEFAULT '[]',
    enabled      INTEGER NOT NULL DEFAULT 1,
    created_at   INTEGER NOT NULL
);
"#,
];

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> anyhow::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> anyhow::Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL; PRAGMA busy_timeout = 5000;",
        )?;
        let version: usize = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version) {
            conn.execute_batch(&format!("BEGIN; {sql}; PRAGMA user_version = {}; COMMIT;", i + 1))?;
        }
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    /// Runs `f` with the connection on tokio's blocking pool.
    pub async fn call<T, F>(&self, f: F) -> DbResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> DbResult<T> + Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = conn.lock().unwrap_or_else(|p| p.into_inner());
            f(&mut guard)
        })
        .await
        .expect("database task panicked")
    }
}

// ---------- users & sessions ----------

pub struct UserRecord {
    pub user: User,
    pub password_hash: String,
}

fn user_from_row(r: &Row) -> DbResult<User> {
    Ok(User { id: r.get(0)?, username: r.get(1)?, role: Role::parse(&r.get::<_, String>(2)?) })
}

pub fn user_count(c: &Connection) -> DbResult<i64> {
    c.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))
}

pub fn create_user(c: &Connection, username: &str, password_hash: &str, role: Role) -> DbResult<User> {
    c.execute(
        "INSERT INTO users (username, password_hash, role, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![username, password_hash, role.as_str(), now()],
    )?;
    Ok(User { id: c.last_insert_rowid(), username: username.to_string(), role })
}

pub fn find_user_by_name(c: &Connection, username: &str) -> DbResult<Option<UserRecord>> {
    c.query_row("SELECT id, username, role, password_hash FROM users WHERE username = ?1", [username], |r| {
        Ok(UserRecord { user: user_from_row(r)?, password_hash: r.get(3)? })
    })
    .optional()
}

pub fn password_hash(c: &Connection, user_id: i64) -> DbResult<String> {
    c.query_row("SELECT password_hash FROM users WHERE id = ?1", [user_id], |r| r.get(0))
}

pub fn set_password_hash(c: &Connection, user_id: i64, hash: &str) -> DbResult<()> {
    c.execute("UPDATE users SET password_hash = ?2 WHERE id = ?1", params![user_id, hash])?;
    Ok(())
}

pub fn create_session(c: &Connection, token_hash: &str, user_id: i64, user_agent: &str, ttl_secs: i64) -> DbResult<()> {
    let t = now();
    c.execute(
        "INSERT INTO sessions (token_hash, user_id, user_agent, created_at, last_seen, expires_at) VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
        params![token_hash, user_id, user_agent, t, t + ttl_secs],
    )?;
    Ok(())
}

/// Looks up a live session and refreshes its `last_seen` (at most once a minute).
pub fn session_user(c: &Connection, token_hash: &str) -> DbResult<Option<(i64, User)>> {
    let t = now();
    let found = c
        .query_row(
            "SELECT s.id, u.id, u.username, u.role, s.last_seen FROM sessions s JOIN users u ON u.id = s.user_id
             WHERE s.token_hash = ?1 AND s.expires_at > ?2",
            params![token_hash, t],
            |r| {
                let user = User { id: r.get(1)?, username: r.get(2)?, role: Role::parse(&r.get::<_, String>(3)?) };
                Ok((r.get::<_, i64>(0)?, user, r.get::<_, i64>(4)?))
            },
        )
        .optional()?;
    Ok(found.map(|(sid, user, last_seen)| {
        if t - last_seen > 60 {
            let _ = c.execute("UPDATE sessions SET last_seen = ?2 WHERE id = ?1", params![sid, t]);
        }
        (sid, user)
    }))
}

pub fn delete_session(c: &Connection, token_hash: &str) -> DbResult<()> {
    c.execute("DELETE FROM sessions WHERE token_hash = ?1", [token_hash])?;
    Ok(())
}

pub fn delete_session_by_id(c: &Connection, user_id: i64, session_id: i64) -> DbResult<bool> {
    Ok(c.execute("DELETE FROM sessions WHERE id = ?1 AND user_id = ?2", params![session_id, user_id])? > 0)
}

pub fn delete_other_sessions(c: &Connection, user_id: i64, keep_session_id: i64) -> DbResult<()> {
    c.execute("DELETE FROM sessions WHERE user_id = ?1 AND id != ?2", params![user_id, keep_session_id])?;
    Ok(())
}

pub fn purge_expired_sessions(c: &Connection) -> DbResult<()> {
    c.execute("DELETE FROM sessions WHERE expires_at <= ?1", [now()])?;
    Ok(())
}

pub fn list_sessions(c: &Connection, user_id: i64, current_session_id: i64) -> DbResult<Vec<DeviceSession>> {
    let mut st = c.prepare(
        "SELECT id, user_agent, created_at, last_seen FROM sessions WHERE user_id = ?1 AND expires_at > ?2 ORDER BY last_seen DESC",
    )?;
    let rows = st.query_map(params![user_id, now()], |r| {
        let id: i64 = r.get(0)?;
        Ok(DeviceSession {
            id,
            user_agent: r.get(1)?,
            created_at: r.get(2)?,
            last_seen: r.get(3)?,
            current: id == current_session_id,
        })
    })?;
    rows.collect()
}

// ---------- providers ----------

pub struct ProviderRecord {
    pub provider: Provider,
    pub api_key_enc: Option<Vec<u8>>,
}

const PROVIDER_COLS: &str = "id, name, kind, base_url, api_key_enc, key_hint";

fn provider_from_row(r: &Row) -> DbResult<ProviderRecord> {
    let kind: String = r.get(2)?;
    let api_key_enc: Option<Vec<u8>> = r.get(4)?;
    Ok(ProviderRecord {
        provider: Provider {
            id: r.get(0)?,
            name: r.get(1)?,
            kind: ProviderKind::parse(&kind).unwrap_or(ProviderKind::OpenAiCompatible),
            base_url: r.get(3)?,
            has_key: api_key_enc.is_some(),
            key_hint: r.get(5)?,
        },
        api_key_enc,
    })
}

pub fn list_providers(c: &Connection, user_id: i64) -> DbResult<Vec<Provider>> {
    let mut st = c.prepare(&format!("SELECT {PROVIDER_COLS} FROM providers WHERE user_id = ?1 ORDER BY id"))?;
    let rows = st.query_map([user_id], |r| provider_from_row(r).map(|p| p.provider))?;
    rows.collect()
}

pub fn get_provider(c: &Connection, user_id: i64, id: i64) -> DbResult<Option<ProviderRecord>> {
    c.query_row(
        &format!("SELECT {PROVIDER_COLS} FROM providers WHERE id = ?1 AND user_id = ?2"),
        params![id, user_id],
        provider_from_row,
    )
    .optional()
}

pub fn insert_provider(
    c: &Connection,
    user_id: i64,
    name: &str,
    kind: ProviderKind,
    base_url: &str,
    key: Option<(Vec<u8>, String)>,
) -> DbResult<i64> {
    let (enc, hint) = key.map(|(e, h)| (Some(e), h)).unwrap_or((None, String::new()));
    c.execute(
        "INSERT INTO providers (user_id, name, kind, base_url, api_key_enc, key_hint, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![user_id, name, kind.as_str(), base_url, enc, hint, now()],
    )?;
    Ok(c.last_insert_rowid())
}

/// `key`: `None` leaves the stored key alone, `Some(None)` clears it, `Some(Some(..))` replaces it.
pub fn update_provider(
    c: &Connection,
    user_id: i64,
    id: i64,
    name: &str,
    kind: ProviderKind,
    base_url: &str,
    key: Option<Option<(Vec<u8>, String)>>,
) -> DbResult<bool> {
    let n = c.execute(
        "UPDATE providers SET name = ?3, kind = ?4, base_url = ?5 WHERE id = ?1 AND user_id = ?2",
        params![id, user_id, name, kind.as_str(), base_url],
    )?;
    if n > 0 {
        if let Some(key) = key {
            let (enc, hint) = key.map(|(e, h)| (Some(e), h)).unwrap_or((None, String::new()));
            c.execute(
                "UPDATE providers SET api_key_enc = ?3, key_hint = ?4 WHERE id = ?1 AND user_id = ?2",
                params![id, user_id, enc, hint],
            )?;
        }
    }
    Ok(n > 0)
}

pub fn delete_provider(c: &Connection, user_id: i64, id: i64) -> DbResult<bool> {
    Ok(c.execute("DELETE FROM providers WHERE id = ?1 AND user_id = ?2", params![id, user_id])? > 0)
}

// ---------- conversations & messages ----------

fn message_from_row(r: &Row) -> DbResult<Message> {
    Ok(Message {
        id: r.get(0)?,
        role: MsgRole::parse(&r.get::<_, String>(1)?),
        content: r.get(2)?,
        provider_id: r.get(3)?,
        model: r.get(4)?,
        error: r.get(5)?,
        created_at: r.get(6)?,
        attachments: Vec::new(),
        tool_calls: serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or_default(),
    })
}

const MESSAGE_COLS: &str = "id, role, content, provider_id, model, error, created_at, tool_calls";

pub fn list_conversations(c: &Connection, user_id: i64) -> DbResult<Vec<ConversationSummary>> {
    let mut st = c.prepare(
        "SELECT id, title, updated_at FROM conversations WHERE user_id = ?1 ORDER BY updated_at DESC, id DESC",
    )?;
    let rows = st
        .query_map([user_id], |r| Ok(ConversationSummary { id: r.get(0)?, title: r.get(1)?, updated_at: r.get(2)? }))?;
    rows.collect()
}

pub fn create_conversation(c: &Connection, user_id: i64, title: &str) -> DbResult<i64> {
    let t = now();
    c.execute(
        "INSERT INTO conversations (user_id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
        params![user_id, title, t],
    )?;
    Ok(c.last_insert_rowid())
}

pub fn get_conversation(c: &Connection, user_id: i64, id: i64) -> DbResult<Option<Conversation>> {
    let conv = c
        .query_row(
            "SELECT id, title, provider_id, model, system_prompt, temperature, top_p, max_tokens, created_at, updated_at
             FROM conversations WHERE id = ?1 AND user_id = ?2",
            params![id, user_id],
            |r| {
                Ok(Conversation {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    provider_id: r.get(2)?,
                    model: r.get(3)?,
                    params: ChatParams {
                        system_prompt: r.get(4)?,
                        temperature: r.get::<_, Option<f64>>(5)?.map(|v| v as f32),
                        top_p: r.get::<_, Option<f64>>(6)?.map(|v| v as f32),
                        max_tokens: r.get::<_, Option<i64>>(7)?.map(|v| v.clamp(0, u32::MAX as i64) as u32),
                    },
                    messages: Vec::new(),
                    created_at: r.get(8)?,
                    updated_at: r.get(9)?,
                })
            },
        )
        .optional()?;
    let Some(mut conv) = conv else { return Ok(None) };
    let mut st = c.prepare(&format!("SELECT {MESSAGE_COLS} FROM messages WHERE conversation_id = ?1 ORDER BY id"))?;
    conv.messages = st.query_map([id], message_from_row)?.collect::<DbResult<_>>()?;
    let mut by_message = conversation_attachments(c, id)?;
    for m in &mut conv.messages {
        if let Some(list) = by_message.remove(&m.id) {
            m.attachments = list.into_iter().map(|a| a.info).collect();
        }
    }
    Ok(Some(conv))
}

pub fn rename_conversation(c: &Connection, user_id: i64, id: i64, title: &str) -> DbResult<bool> {
    Ok(c.execute("UPDATE conversations SET title = ?3 WHERE id = ?1 AND user_id = ?2", params![id, user_id, title])?
        > 0)
}

pub fn set_conversation_model(c: &Connection, user_id: i64, id: i64, provider_id: i64, model: &str) -> DbResult<bool> {
    Ok(c.execute(
        "UPDATE conversations SET provider_id = ?3, model = ?4 WHERE id = ?1 AND user_id = ?2",
        params![id, user_id, provider_id, model],
    )? > 0)
}

pub fn set_conversation_params(c: &Connection, user_id: i64, id: i64, p: &ChatParams) -> DbResult<bool> {
    Ok(c.execute(
        "UPDATE conversations SET system_prompt = ?3, temperature = ?4, top_p = ?5, max_tokens = ?6 WHERE id = ?1 AND user_id = ?2",
        params![id, user_id, p.system_prompt, p.temperature.map(f64::from), p.top_p.map(f64::from), p.max_tokens.map(i64::from)],
    )? > 0)
}

/// Deletes a conversation and its attachments. Returns `None` if it doesn't exist, otherwise the
/// attachment file paths to remove from disk.
pub fn delete_conversation(c: &mut Connection, user_id: i64, id: i64) -> DbResult<Option<Vec<String>>> {
    let tx = c.transaction()?;
    let paths: Vec<String> = {
        let mut st = tx.prepare(
            "SELECT a.path FROM attachments a JOIN messages m ON m.id = a.message_id
             JOIN conversations c ON c.id = m.conversation_id WHERE c.id = ?1 AND c.user_id = ?2",
        )?;
        let rows = st.query_map(params![id, user_id], |r| r.get(0))?;
        rows.collect::<DbResult<_>>()?
    };
    tx.execute(
        "DELETE FROM attachments WHERE message_id IN (SELECT m.id FROM messages m JOIN conversations c ON c.id = m.conversation_id WHERE c.id = ?1 AND c.user_id = ?2)",
        params![id, user_id],
    )?;
    let n = tx.execute("DELETE FROM conversations WHERE id = ?1 AND user_id = ?2", params![id, user_id])?;
    tx.commit()?;
    Ok((n > 0).then_some(paths))
}

pub fn touch_conversation(c: &Connection, id: i64) -> DbResult<()> {
    c.execute("UPDATE conversations SET updated_at = ?2 WHERE id = ?1", params![id, now()])?;
    Ok(())
}

pub fn insert_message(
    c: &Connection,
    conversation_id: i64,
    role: MsgRole,
    content: &str,
    provider_id: Option<i64>,
    model: Option<&str>,
) -> DbResult<Message> {
    let t = now();
    c.execute(
        "INSERT INTO messages (conversation_id, role, content, provider_id, model, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![conversation_id, role.as_str(), content, provider_id, model, t],
    )?;
    touch_conversation(c, conversation_id)?;
    Ok(Message {
        id: c.last_insert_rowid(),
        role,
        content: content.to_string(),
        provider_id,
        model: model.map(str::to_string),
        error: None,
        created_at: t,
        attachments: Vec::new(),
        tool_calls: Vec::new(),
    })
}

pub fn finish_message(
    c: &Connection,
    id: i64,
    content: &str,
    error: Option<&str>,
    tool_calls: &[ToolCall],
) -> DbResult<Option<Message>> {
    let calls = serde_json::to_string(tool_calls).unwrap_or_else(|_| "[]".into());
    c.execute(
        "UPDATE messages SET content = ?2, error = ?3, tool_calls = ?4 WHERE id = ?1",
        params![id, content, error, calls],
    )?;
    c.query_row(&format!("SELECT {MESSAGE_COLS} FROM messages WHERE id = ?1"), [id], message_from_row).optional()
}

// ---------- attachments ----------

#[derive(Debug, Clone)]
pub struct AttachmentRecord {
    pub info: Attachment,
    pub path: String,
    pub text: Option<String>,
    pub message_id: Option<i64>,
}

const ATTACHMENT_COLS: &str = "a.id, a.name, a.mime, a.kind, a.size, a.path, a.text, a.message_id";

fn attachment_from_row(r: &Row) -> DbResult<AttachmentRecord> {
    Ok(AttachmentRecord {
        info: Attachment {
            id: r.get(0)?,
            name: r.get(1)?,
            mime: r.get(2)?,
            kind: match r.get::<_, String>(3)?.as_str() {
                "image" => AttachmentKind::Image,
                "pdf" => AttachmentKind::Pdf,
                _ => AttachmentKind::Text,
            },
            size: r.get(4)?,
        },
        path: r.get(5)?,
        text: r.get(6)?,
        message_id: r.get(7)?,
    })
}

fn kind_str(k: AttachmentKind) -> &'static str {
    match k {
        AttachmentKind::Image => "image",
        AttachmentKind::Pdf => "pdf",
        AttachmentKind::Text => "text",
    }
}

#[allow(clippy::too_many_arguments)]
pub fn insert_attachment(
    c: &Connection,
    user_id: i64,
    name: &str,
    mime: &str,
    kind: AttachmentKind,
    size: i64,
    path: &str,
    text: Option<&str>,
) -> DbResult<Attachment> {
    c.execute(
        "INSERT INTO attachments (user_id, name, mime, kind, size, path, text, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![user_id, name, mime, kind_str(kind), size, path, text, now()],
    )?;
    Ok(Attachment { id: c.last_insert_rowid(), name: name.to_string(), mime: mime.to_string(), kind, size })
}

pub fn get_attachment(c: &Connection, user_id: i64, id: i64) -> DbResult<Option<AttachmentRecord>> {
    c.query_row(
        &format!("SELECT {ATTACHMENT_COLS} FROM attachments a WHERE a.id = ?1 AND a.user_id = ?2"),
        params![id, user_id],
        attachment_from_row,
    )
    .optional()
}

/// Links uploaded files to a message. Only the owner's files that aren't attached elsewhere qualify.
pub fn link_attachments(c: &Connection, user_id: i64, ids: &[i64], message_id: i64) -> DbResult<usize> {
    let mut n = 0;
    for id in ids {
        n += c.execute(
            "UPDATE attachments SET message_id = ?3, created_at = ?4 WHERE id = ?1 AND user_id = ?2 AND message_id IS NULL",
            params![id, user_id, message_id, now()],
        )?;
    }
    Ok(n)
}

pub fn conversation_attachments(
    c: &Connection,
    conversation_id: i64,
) -> DbResult<std::collections::HashMap<i64, Vec<AttachmentRecord>>> {
    let mut st = c.prepare(&format!(
        "SELECT {ATTACHMENT_COLS} FROM attachments a JOIN messages m ON m.id = a.message_id WHERE m.conversation_id = ?1 ORDER BY a.id"
    ))?;
    let mut map: std::collections::HashMap<i64, Vec<AttachmentRecord>> = Default::default();
    for rec in st.query_map([conversation_id], attachment_from_row)? {
        let rec = rec?;
        if let Some(mid) = rec.message_id {
            map.entry(mid).or_default().push(rec);
        }
    }
    Ok(map)
}

pub fn delete_attachment(c: &Connection, user_id: i64, id: i64) -> DbResult<Option<String>> {
    let path: Option<String> = c
        .query_row(
            "SELECT path FROM attachments WHERE id = ?1 AND user_id = ?2 AND message_id IS NULL",
            params![id, user_id],
            |r| r.get(0),
        )
        .optional()?;
    if path.is_some() {
        c.execute("DELETE FROM attachments WHERE id = ?1", [id])?;
    }
    Ok(path)
}

/// Removes uploads that were never sent (or whose message was deleted) before `cutoff`.
pub fn sweep_orphan_attachments(c: &Connection, cutoff: i64) -> DbResult<Vec<String>> {
    let mut st = c.prepare("SELECT path FROM attachments WHERE message_id IS NULL AND created_at < ?1")?;
    let paths: Vec<String> = st.query_map([cutoff], |r| r.get(0))?.collect::<DbResult<_>>()?;
    c.execute("DELETE FROM attachments WHERE message_id IS NULL AND created_at < ?1", [cutoff])?;
    Ok(paths)
}

pub fn all_attachment_paths(c: &Connection) -> DbResult<std::collections::HashSet<String>> {
    let mut st = c.prepare("SELECT path FROM attachments")?;
    let rows = st.query_map([], |r| r.get(0))?;
    rows.collect()
}

// ---------- search settings ----------

pub struct SearchRecord {
    pub settings: SearchSettings,
    pub api_key_enc: Option<Vec<u8>>,
}

pub fn search_settings_record(c: &Connection, user_id: i64) -> DbResult<SearchRecord> {
    let rec = c
        .query_row(
            "SELECT backend, base_url, api_key_enc, key_hint FROM search_settings WHERE user_id = ?1",
            [user_id],
            |r| {
                let enc: Option<Vec<u8>> = r.get(2)?;
                Ok(SearchRecord {
                    settings: SearchSettings {
                        backend: SearchBackend::parse(&r.get::<_, String>(0)?),
                        base_url: r.get(1)?,
                        has_key: enc.is_some(),
                        key_hint: r.get(3)?,
                    },
                    api_key_enc: enc,
                })
            },
        )
        .optional()?;
    Ok(rec.unwrap_or(SearchRecord { settings: SearchSettings::default(), api_key_enc: None }))
}

/// `key`: `None` keeps the stored key, `Some(None)` clears it.
pub fn save_search_settings(
    c: &Connection,
    user_id: i64,
    backend: SearchBackend,
    base_url: &str,
    key: Option<Option<(Vec<u8>, String)>>,
) -> DbResult<()> {
    c.execute(
        "INSERT INTO search_settings (user_id, backend, base_url) VALUES (?1, ?2, ?3)
         ON CONFLICT(user_id) DO UPDATE SET backend = excluded.backend, base_url = excluded.base_url",
        params![user_id, backend.as_str(), base_url],
    )?;
    if let Some(key) = key {
        let (enc, hint) = key.map(|(e, h)| (Some(e), h)).unwrap_or((None, String::new()));
        c.execute(
            "UPDATE search_settings SET api_key_enc = ?2, key_hint = ?3 WHERE user_id = ?1",
            params![user_id, enc, hint],
        )?;
    }
    Ok(())
}

// ---------- MCP servers ----------

pub struct McpRecord {
    pub server: McpServer,
    pub headers_enc: Option<Vec<u8>>,
    pub env_enc: Option<Vec<u8>>,
}

impl McpRecord {
    pub fn decrypt(&self, vault: &crate::crypto::Vault) -> Result<crate::tools::mcp::McpConfig, String> {
        let open = |b: &Option<Vec<u8>>| -> Result<Vec<(String, String)>, String> {
            match b {
                Some(b) => {
                    serde_json::from_str(&vault.decrypt(b).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
                }
                None => Ok(Vec::new()),
            }
        };
        Ok(crate::tools::mcp::McpConfig {
            id: self.server.id,
            name: self.server.name.clone(),
            transport: self.server.transport,
            url: self.server.url.clone(),
            headers: open(&self.headers_enc)?,
            command: self.server.command.clone(),
            args: self.server.args.clone(),
            env: open(&self.env_enc)?,
        })
    }
}

const MCP_COLS: &str =
    "id, name, transport, url, command, args, header_names, env_names, enabled, headers_enc, env_enc";

fn mcp_from_row(r: &Row) -> DbResult<McpRecord> {
    let json_list =
        |i: usize| -> DbResult<Vec<String>> { Ok(serde_json::from_str(&r.get::<_, String>(i)?).unwrap_or_default()) };
    Ok(McpRecord {
        server: McpServer {
            id: r.get(0)?,
            name: r.get(1)?,
            transport: McpTransport::parse(&r.get::<_, String>(2)?),
            url: r.get(3)?,
            command: r.get(4)?,
            args: json_list(5)?,
            header_names: json_list(6)?,
            env_names: json_list(7)?,
            enabled: r.get(8)?,
        },
        headers_enc: r.get(9)?,
        env_enc: r.get(10)?,
    })
}

pub fn list_mcp_servers(c: &Connection, user_id: i64) -> DbResult<Vec<McpServer>> {
    let mut st = c.prepare(&format!("SELECT {MCP_COLS} FROM mcp_servers WHERE user_id = ?1 ORDER BY id"))?;
    let rows = st.query_map([user_id], |r| mcp_from_row(r).map(|m| m.server))?;
    rows.collect()
}

pub fn get_mcp_record(c: &Connection, user_id: i64, id: i64) -> DbResult<Option<McpRecord>> {
    c.query_row(
        &format!("SELECT {MCP_COLS} FROM mcp_servers WHERE id = ?1 AND user_id = ?2"),
        params![id, user_id],
        mcp_from_row,
    )
    .optional()
}

pub fn mcp_records(c: &Connection, user_id: i64, ids: &[i64]) -> DbResult<Vec<McpRecord>> {
    let mut out = Vec::new();
    for id in ids {
        if let Some(r) = get_mcp_record(c, user_id, *id)? {
            out.push(r);
        }
    }
    Ok(out)
}

/// Encrypted name/value pairs plus the list of names (shown in the UI).
pub type SecretPairs = (Vec<u8>, Vec<String>);

#[allow(clippy::too_many_arguments)]
pub fn save_mcp_server(
    c: &Connection,
    user_id: i64,
    id: Option<i64>,
    name: &str,
    transport: McpTransport,
    url: &str,
    command: &str,
    args: &[String],
    headers: Option<SecretPairs>,
    env: Option<SecretPairs>,
    enabled: bool,
) -> DbResult<Option<i64>> {
    let args = serde_json::to_string(args).unwrap_or_else(|_| "[]".into());
    let id = match id {
        Some(id) => {
            let n = c.execute(
                "UPDATE mcp_servers SET name = ?3, transport = ?4, url = ?5, command = ?6, args = ?7, enabled = ?8 WHERE id = ?1 AND user_id = ?2",
                params![id, user_id, name, transport.as_str(), url, command, args, enabled],
            )?;
            if n == 0 {
                return Ok(None);
            }
            id
        }
        None => {
            c.execute(
                "INSERT INTO mcp_servers (user_id, name, transport, url, command, args, enabled, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![user_id, name, transport.as_str(), url, command, args, enabled, now()],
            )?;
            c.last_insert_rowid()
        }
    };
    if let Some((enc, names)) = headers {
        c.execute(
            "UPDATE mcp_servers SET headers_enc = ?2, header_names = ?3 WHERE id = ?1",
            params![id, enc, serde_json::to_string(&names).unwrap_or_default()],
        )?;
    }
    if let Some((enc, names)) = env {
        c.execute(
            "UPDATE mcp_servers SET env_enc = ?2, env_names = ?3 WHERE id = ?1",
            params![id, enc, serde_json::to_string(&names).unwrap_or_default()],
        )?;
    }
    Ok(Some(id))
}

pub fn delete_mcp_server(c: &Connection, user_id: i64, id: i64) -> DbResult<bool> {
    Ok(c.execute("DELETE FROM mcp_servers WHERE id = ?1 AND user_id = ?2", params![id, user_id])? > 0)
}

/// Deletes a message and every later message in its conversation (used by "regenerate").
pub fn truncate_from(c: &Connection, user_id: i64, conversation_id: i64, message_id: i64) -> DbResult<bool> {
    let owned: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM conversations WHERE id = ?1 AND user_id = ?2)",
        params![conversation_id, user_id],
        |r| r.get(0),
    )?;
    if !owned {
        return Ok(false);
    }
    c.execute("DELETE FROM messages WHERE conversation_id = ?1 AND id >= ?2", params![conversation_id, message_id])?;
    Ok(true)
}

/// Turns free text into a safe FTS5 query: every word becomes a quoted prefix term.
pub fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split_whitespace()
        .map(|w| w.replace('"', ""))
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

pub fn search(c: &Connection, user_id: i64, q: &str) -> DbResult<Vec<SearchHit>> {
    let mut hits: Vec<SearchHit> = Vec::new();
    let like = format!("%{}%", q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
    let mut st = c.prepare(
        "SELECT id, title FROM conversations WHERE user_id = ?1 AND title LIKE ?2 ESCAPE '\\' ORDER BY updated_at DESC LIMIT 20",
    )?;
    for row in st.query_map(params![user_id, like], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
        let (id, title) = row?;
        hits.push(SearchHit { conversation_id: id, snippet: title.clone(), title });
    }
    if let Some(fq) = fts_query(q) {
        let mut st = c.prepare(
            "SELECT c.id, c.title, snippet(messages_fts, 0, '[[', ']]', '…', 12)
             FROM messages_fts JOIN messages m ON m.id = messages_fts.rowid JOIN conversations c ON c.id = m.conversation_id
             WHERE messages_fts MATCH ?2 AND c.user_id = ?1 ORDER BY rank LIMIT 50",
        )?;
        for row in st.query_map(params![user_id, fq], |r| {
            Ok(SearchHit { conversation_id: r.get(0)?, title: r.get(1)?, snippet: r.get(2)? })
        })? {
            let hit = row?;
            if !hits.iter().any(|h| h.conversation_id == hit.conversation_id) {
                hits.push(hit);
            }
        }
    }
    hits.truncate(30);
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Connection, i64) {
        let db = Db::open_in_memory().unwrap();
        let conn = Arc::try_unwrap(db.conn).ok().unwrap().into_inner().unwrap();
        let u = create_user(&conn, "neko", "hash", Role::Admin).unwrap();
        (conn, u.id)
    }

    #[test]
    fn conversation_flow_and_search() {
        let (mut c, uid) = setup();
        let cid = create_conversation(&c, uid, "Cats & code").unwrap();
        insert_message(&c, cid, MsgRole::User, "How do I write a Rust iterator?", None, None).unwrap();
        let a = insert_message(&c, cid, MsgRole::Assistant, "", Some(1), Some("m")).unwrap();
        finish_message(&c, a.id, "Implement the Iterator trait with fn next.", None, &[]).unwrap();

        let conv = get_conversation(&c, uid, cid).unwrap().unwrap();
        assert_eq!(conv.messages.len(), 2);
        assert_eq!(conv.messages[1].content, "Implement the Iterator trait with fn next.");

        let hits = search(&c, uid, "iterat").unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].snippet.contains("[["));
        assert_eq!(search(&c, uid, "cats").unwrap()[0].conversation_id, cid);
        // Quotes and FTS operators must not break the query.
        assert!(search(&c, uid, "\"NEAR( OR *").is_ok());
        assert!(search(&c, uid + 1, "iterator").unwrap().is_empty());

        assert!(truncate_from(&c, uid, cid, a.id).unwrap());
        assert_eq!(get_conversation(&c, uid, cid).unwrap().unwrap().messages.len(), 1);
        assert!(search(&c, uid, "trait").unwrap().is_empty());
        assert!(delete_conversation(&mut c, uid, cid).unwrap().is_some());
        assert!(get_conversation(&c, uid, cid).unwrap().is_none());
    }

    #[test]
    fn fts_query_quotes_terms() {
        assert_eq!(fts_query("a \"b c"), Some("\"a\"* \"b\"* \"c\"*".into()));
        assert_eq!(fts_query("   "), None);
    }
}
