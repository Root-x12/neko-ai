//! Types shared between the Neko AI server and the web client.

use serde::{Deserialize, Serialize};

pub mod sse;

// ---------- auth ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Runs the server: users, workspaces, every audit entry.
    Admin,
    /// Manages their own workspace's policies and sees its audit log.
    Team,
    User,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Team => "team",
            Role::User => "user",
        }
    }

    pub fn parse(s: &str) -> Role {
        match s {
            "admin" => Role::Admin,
            "team" => Role::Team,
            _ => Role::User,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Role::Admin => "Admin",
            Role::Team => "Team lead",
            Role::User => "User",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub role: Role,
    #[serde(default = "default_workspace")]
    pub workspace_id: i64,
}

fn default_workspace() -> i64 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthStatus {
    /// No account exists yet: the first sign-up becomes the admin and needs the setup code.
    pub needs_setup: bool,
    pub signup_open: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
    /// Required only for the very first account (printed in the server console).
    #[serde(default)]
    pub setup_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

// ---------- providers ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// Anything that speaks the OpenAI `/chat/completions` API: OpenAI, OpenRouter, Groq,
    /// Mistral, DeepSeek, xAI, Together, Ollama, LM Studio, vLLM...
    OpenAiCompatible,
    Anthropic,
    Gemini,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::OpenAiCompatible => "open_ai_compatible",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::Gemini => "gemini",
        }
    }

    pub fn parse(s: &str) -> Option<ProviderKind> {
        match s {
            "open_ai_compatible" => Some(ProviderKind::OpenAiCompatible),
            "anthropic" => Some(ProviderKind::Anthropic),
            "gemini" => Some(ProviderKind::Gemini),
            _ => None,
        }
    }
}

/// A ready-made provider template shown in the "add provider" form.
pub struct ProviderPreset {
    pub name: &'static str,
    pub kind: ProviderKind,
    pub base_url: &'static str,
    pub needs_key: bool,
}

pub const PROVIDER_PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        name: "OpenAI",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "https://api.openai.com/v1",
        needs_key: true,
    },
    ProviderPreset {
        name: "Anthropic",
        kind: ProviderKind::Anthropic,
        base_url: "https://api.anthropic.com",
        needs_key: true,
    },
    ProviderPreset {
        name: "Google Gemini",
        kind: ProviderKind::Gemini,
        base_url: "https://generativelanguage.googleapis.com",
        needs_key: true,
    },
    ProviderPreset {
        name: "OpenRouter",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "https://openrouter.ai/api/v1",
        needs_key: true,
    },
    ProviderPreset {
        name: "Groq",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "https://api.groq.com/openai/v1",
        needs_key: true,
    },
    ProviderPreset {
        name: "Mistral",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "https://api.mistral.ai/v1",
        needs_key: true,
    },
    ProviderPreset {
        name: "DeepSeek",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "https://api.deepseek.com/v1",
        needs_key: true,
    },
    ProviderPreset {
        name: "xAI",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "https://api.x.ai/v1",
        needs_key: true,
    },
    ProviderPreset {
        name: "Together",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "https://api.together.xyz/v1",
        needs_key: true,
    },
    ProviderPreset {
        name: "Ollama (local)",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "http://localhost:11434/v1",
        needs_key: false,
    },
    ProviderPreset {
        name: "LM Studio (local)",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "http://localhost:1234/v1",
        needs_key: false,
    },
    ProviderPreset {
        name: "Custom (OpenAI-compatible)",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "",
        needs_key: false,
    },
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provider {
    pub id: i64,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub has_key: bool,
    /// Last few characters of the key, e.g. "…a1b2". The key itself never leaves the server.
    pub key_hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInput {
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    /// `None` keeps the stored key on update; `Some("")` removes it.
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelList {
    pub models: Vec<String>,
}

// ---------- conversations ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MsgRole {
    User,
    Assistant,
}

impl MsgRole {
    pub fn as_str(self) -> &'static str {
        match self {
            MsgRole::User => "user",
            MsgRole::Assistant => "assistant",
        }
    }

    pub fn parse(s: &str) -> MsgRole {
        match s {
            "assistant" => MsgRole::Assistant,
            _ => MsgRole::User,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ChatParams {
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub role: MsgRole,
    pub content: String,
    pub provider_id: Option<i64>,
    pub model: Option<String>,
    /// Set when generation failed; `content` then holds whatever arrived before the error.
    pub error: Option<String>,
    pub created_at: i64,
    /// Files attached to a user message.
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    /// Tools the assistant used while writing this answer, in order.
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttachmentKind {
    Image,
    Pdf,
    Text,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    pub id: i64,
    pub name: String,
    pub mime: String,
    pub kind: AttachmentKind,
    pub size: i64,
    /// Prompt-injection warnings found in the file's text.
    #[serde(default)]
    pub flags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    /// Name the model used, e.g. `web_search` or `mcp3__read_file`.
    pub name: String,
    /// Human-friendly label, e.g. "Web search" or "files › read_file".
    pub label: String,
    /// JSON arguments as sent by the model.
    pub arguments: String,
    /// Result text (may be truncated for display). Empty while running.
    #[serde(default)]
    pub result: String,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub done: bool,
    /// Prompt-injection warnings found in the result.
    #[serde(default)]
    pub flags: Vec<String>,
}

/// Which tools the model may use for one request.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ToolSelection {
    #[serde(default)]
    pub web_search: bool,
    #[serde(default)]
    pub fetch_url: bool,
    #[serde(default)]
    pub mcp_servers: Vec<i64>,
    /// Search the user's knowledge library.
    #[serde(default)]
    pub knowledge: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub id: i64,
    pub title: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    pub id: i64,
    pub title: String,
    pub provider_id: Option<i64>,
    pub model: Option<String>,
    pub params: ChatParams,
    pub messages: Vec<Message>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConversationPatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub provider_id: Option<i64>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub params: Option<ChatParams>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessage {
    pub content: String,
    pub provider_id: i64,
    pub model: String,
    #[serde(default)]
    pub params: ChatParams,
    /// Edit-and-resend: delete this message and everything after it before sending.
    #[serde(default)]
    pub replace_from: Option<i64>,
    /// Ids returned by `POST /api/uploads`.
    #[serde(default)]
    pub attachments: Vec<i64>,
    /// Send even though the guard found secrets/personal data (only under the "warn" policy).
    #[serde(default)]
    pub allow_findings: bool,
    /// Replace detected secrets/personal data in the message with placeholders before sending.
    #[serde(default)]
    pub redact: bool,
    #[serde(default)]
    pub tools: ToolSelection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Regenerate {
    pub provider_id: i64,
    pub model: String,
    #[serde(default)]
    pub params: ChatParams,
    #[serde(default)]
    pub tools: ToolSelection,
}

/// Events streamed (as SSE `data:` JSON) while the assistant answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// `user_message` is `None` when regenerating an answer.
    Start {
        user_message: Option<Message>,
        assistant_message_id: i64,
        model: String,
    },
    Delta {
        text: String,
    },
    /// The model started a tool; `call.done` is false.
    ToolStart {
        call: ToolCall,
    },
    /// A tool finished; `call` carries the result.
    ToolEnd {
        call: ToolCall,
    },
    Done {
        message: Message,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    pub conversation_id: i64,
    pub title: String,
    /// Plain text excerpt; matched terms are wrapped in `[[` and `]]`.
    pub snippet: String,
}

// ---------- devices ----------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceSession {
    pub id: i64,
    pub user_agent: String,
    pub created_at: i64,
    pub last_seen: i64,
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DevicesInfo {
    /// Addresses other devices on the same network can open.
    pub urls: Vec<String>,
    /// QR code (SVG markup) for the first URL, if any.
    pub qr_svg: Option<String>,
    pub sessions: Vec<DeviceSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
    /// Present when a message was held back because it contains secrets or personal data.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<Finding>,
}

// ---------- security & governance ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuardPolicy {
    Off,
    /// Ask before sending (the user can send anyway or redact).
    #[default]
    Warn,
    /// Replace matches with placeholders automatically.
    Redact,
    /// Refuse to send.
    Block,
}

impl GuardPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            GuardPolicy::Off => "off",
            GuardPolicy::Warn => "warn",
            GuardPolicy::Redact => "redact",
            GuardPolicy::Block => "block",
        }
    }

    pub fn parse(s: &str) -> GuardPolicy {
        match s {
            "off" => GuardPolicy::Off,
            "redact" => GuardPolicy::Redact,
            "block" => GuardPolicy::Block,
            _ => GuardPolicy::Warn,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    /// e.g. "OpenAI API key", "Email address".
    pub kind: String,
    /// `true` for credentials, `false` for personal data.
    pub secret: bool,
    /// Masked excerpt, e.g. "sk-p…9Xa".
    pub sample: String,
    /// "message" or the attachment's file name.
    pub location: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: i64,
    pub name: String,
    /// Model name patterns (`*` wildcard). Empty = every model allowed.
    pub model_allowlist: Vec<String>,
    /// Chats untouched for this many days are deleted. `None` keeps them forever.
    pub retention_days: Option<u32>,
    pub secret_policy: GuardPolicy,
    pub pii_policy: GuardPolicy,
    /// Flag fetched pages, tool results and uploads that try to instruct the AI.
    pub injection_scan: bool,
    #[serde(default)]
    pub member_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInput {
    pub name: String,
    #[serde(default)]
    pub model_allowlist: Vec<String>,
    #[serde(default)]
    pub retention_days: Option<u32>,
    #[serde(default)]
    pub secret_policy: GuardPolicy,
    #[serde(default)]
    pub pii_policy: GuardPolicy,
    #[serde(default = "yes")]
    pub injection_scan: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdminUser {
    pub id: i64,
    pub username: String,
    pub role: Role,
    pub workspace_id: i64,
    pub disabled: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUser {
    pub username: String,
    pub password: String,
    pub role: Role,
    pub workspace_id: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateUser {
    #[serde(default)]
    pub role: Option<Role>,
    #[serde(default)]
    pub workspace_id: Option<i64>,
    #[serde(default)]
    pub disabled: Option<bool>,
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: i64,
    pub ts: i64,
    pub username: String,
    pub workspace_id: Option<i64>,
    pub action: String,
    pub target: String,
    pub detail: String,
    pub ip: String,
}

/// What the signed-in user's environment allows, for the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub version: String,
    /// Nothing leaves the local network: only LAN/loopback providers and MCP servers, no web tools.
    pub local_only: bool,
    pub workspace: Workspace,
}

// ---------- tools settings ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchBackend {
    /// No key needed; scrapes DuckDuckGo's HTML page (best effort).
    #[default]
    DuckDuckGo,
    /// Your own SearXNG instance (JSON format must be enabled).
    Searxng,
    Brave,
    Tavily,
}

impl SearchBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            SearchBackend::DuckDuckGo => "duck_duck_go",
            SearchBackend::Searxng => "searxng",
            SearchBackend::Brave => "brave",
            SearchBackend::Tavily => "tavily",
        }
    }

    pub fn parse(s: &str) -> SearchBackend {
        match s {
            "searxng" => SearchBackend::Searxng,
            "brave" => SearchBackend::Brave,
            "tavily" => SearchBackend::Tavily,
            _ => SearchBackend::DuckDuckGo,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SearchSettings {
    pub backend: SearchBackend,
    /// SearXNG instance URL.
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub has_key: bool,
    #[serde(default)]
    pub key_hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchSettingsInput {
    pub backend: SearchBackend,
    #[serde(default)]
    pub base_url: String,
    /// `None` keeps the stored key; `Some("")` removes it.
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpTransport {
    /// Streamable HTTP (remote or local URL).
    Http,
    /// A local command speaking MCP over stdin/stdout. Admin only: it runs on the server.
    Stdio,
}

impl McpTransport {
    pub fn as_str(self) -> &'static str {
        match self {
            McpTransport::Http => "http",
            McpTransport::Stdio => "stdio",
        }
    }

    pub fn parse(s: &str) -> McpTransport {
        if s == "stdio" {
            McpTransport::Stdio
        } else {
            McpTransport::Http
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpServer {
    pub id: i64,
    pub name: String,
    pub transport: McpTransport,
    pub url: String,
    pub command: String,
    pub args: Vec<String>,
    /// Names only; values (often secrets) never leave the server.
    pub header_names: Vec<String>,
    pub env_names: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerInput {
    pub name: String,
    pub transport: McpTransport,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// `None` keeps stored headers; `Some(list)` replaces them.
    #[serde(default)]
    pub headers: Option<Vec<(String, String)>>,
    /// `None` keeps stored env vars; `Some(list)` replaces them.
    #[serde(default)]
    pub env: Option<Vec<(String, String)>>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpTestResult {
    pub server_name: String,
    pub tools: Vec<McpToolInfo>,
}

// ---------- memory, knowledge, export ----------

/// Something the assistant should remember about the user across chats.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    pub id: i64,
    pub content: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryInput {
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MemorySettings {
    /// Include saved memories in every chat.
    pub enabled: bool,
    /// Let the AI save new memories itself (needs a model with tool support).
    pub auto_save: bool,
}

impl Default for MemorySettings {
    fn default() -> Self {
        Self { enabled: true, auto_save: false }
    }
}

/// A file in the user's knowledge library, searchable by the AI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub id: i64,
    pub name: String,
    pub mime: String,
    pub size: i64,
    pub chunks: i64,
    #[serde(default)]
    pub flags: Vec<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeHit {
    pub document_id: i64,
    pub document: String,
    pub chunk: i64,
    pub text: String,
}

/// Everything a user owns, for `GET /api/export`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataExport {
    pub format: String,
    pub exported_at: i64,
    pub user: User,
    pub conversations: Vec<Conversation>,
    pub memories: Vec<Memory>,
    pub documents: Vec<Document>,
}

/// A user's own background picture for one palette (slot = palette id, e.g. "sakura").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Wallpaper {
    pub slot: String,
    pub updated_at: i64,
}
