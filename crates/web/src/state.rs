//! App-wide state and the actions that change it.

use std::{cell::Cell, collections::HashMap, rc::Rc, time::Duration};

use leptos::{prelude::*, task::spawn_local};
use neko_shared::*;
use serde::Serialize;

use crate::{api, theme, util};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingsTab {
    Appearance,
    Providers,
    Tools,
    Devices,
    Account,
}

#[derive(Clone, PartialEq)]
pub struct Toast {
    pub id: u32,
    pub text: String,
    pub error: bool,
}

/// One rendered chat message. Fields are signals so streaming updates only touch that message.
#[derive(Clone, Copy)]
pub struct MsgView {
    pub id: i64,
    pub role: MsgRole,
    pub content: RwSignal<String>,
    pub model: RwSignal<Option<String>>,
    pub error: RwSignal<Option<String>>,
    pub streaming: RwSignal<bool>,
    pub attachments: StoredValue<Vec<Attachment>>,
    pub tool_calls: RwSignal<Vec<ToolCall>>,
}

impl MsgView {
    pub fn from_message(m: Message) -> Self {
        Self {
            id: m.id,
            role: m.role,
            content: RwSignal::new(m.content),
            model: RwSignal::new(m.model),
            error: RwSignal::new(m.error),
            streaming: RwSignal::new(false),
            attachments: StoredValue::new(m.attachments),
            tool_calls: RwSignal::new(m.tool_calls),
        }
    }
}

/// A file picked in the composer: uploading (`att` is None) or ready to send.
#[derive(Clone, PartialEq)]
pub struct PendingFile {
    pub key: u32,
    pub name: String,
    pub att: Option<Attachment>,
}

const TOOLS_KEY: &str = "neko.tools";

const LAST_MODEL_KEY: &str = "neko.lastModel";

#[derive(Clone, Copy)]
pub struct Ctx {
    pub user: RwSignal<User>,
    pub providers: RwSignal<Vec<Provider>>,
    pub providers_loaded: RwSignal<bool>,
    pub models: RwSignal<HashMap<i64, Vec<String>>>,
    pub models_loading: RwSignal<bool>,
    pub conversations: RwSignal<Vec<ConversationSummary>>,
    pub current_id: RwSignal<Option<i64>>,
    pub current_title: RwSignal<String>,
    pub messages: RwSignal<Vec<MsgView>>,
    pub loading_chat: RwSignal<bool>,
    pub provider_id: RwSignal<Option<i64>>,
    pub model: RwSignal<String>,
    pub params: RwSignal<ChatParams>,
    pub streaming: RwSignal<bool>,
    abort: StoredValue<Option<web_sys::AbortController>, LocalStorage>,
    /// Edit-and-resend: the user message being replaced.
    pub editing: RwSignal<Option<i64>>,
    pub draft: RwSignal<String>,
    pub sidebar_open: RwSignal<bool>,
    pub settings: RwSignal<Option<SettingsTab>>,
    pub params_open: RwSignal<bool>,
    pub toast: RwSignal<Option<Toast>>,
    pub idle: RwSignal<bool>,
    pub signed_out: RwSignal<bool>,
    pub theme: theme::Theme,
    pub pending: RwSignal<Vec<PendingFile>>,
    pub tools: RwSignal<ToolSelection>,
    pub mcp_servers: RwSignal<Vec<McpServer>>,
}

impl Ctx {
    pub fn new(user: User, theme: theme::Theme) -> Self {
        Self {
            user: RwSignal::new(user),
            providers: RwSignal::new(Vec::new()),
            providers_loaded: RwSignal::new(false),
            models: RwSignal::new(HashMap::new()),
            models_loading: RwSignal::new(false),
            conversations: RwSignal::new(Vec::new()),
            current_id: RwSignal::new(None),
            current_title: RwSignal::new(String::new()),
            messages: RwSignal::new(Vec::new()),
            loading_chat: RwSignal::new(false),
            provider_id: RwSignal::new(None),
            model: RwSignal::new(String::new()),
            params: RwSignal::new(ChatParams::default()),
            streaming: RwSignal::new(false),
            abort: StoredValue::new_local(None),
            editing: RwSignal::new(None),
            draft: RwSignal::new(String::new()),
            sidebar_open: RwSignal::new(false),
            settings: RwSignal::new(None),
            params_open: RwSignal::new(false),
            toast: RwSignal::new(None),
            idle: RwSignal::new(false),
            signed_out: RwSignal::new(false),
            theme,
            pending: RwSignal::new(Vec::new()),
            tools: RwSignal::new(
                theme::load_pref(TOOLS_KEY).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
            ),
            mcp_servers: RwSignal::new(Vec::new()),
        }
    }

    pub fn notify(self, text: impl Into<String>, error: bool) {
        let id = self.toast.with_untracked(|t| t.as_ref().map_or(0, |t| t.id)) + 1;
        self.toast.set(Some(Toast { id, text: text.into(), error }));
        let toast = self.toast;
        set_timeout(
            move || {
                if toast.with_untracked(|t| t.as_ref().is_some_and(|t| t.id == id)) {
                    toast.set(None);
                }
            },
            Duration::from_millis(if error { 6000 } else { 2500 }),
        );
    }

    pub fn error(self, e: impl std::fmt::Display) {
        let msg = e.to_string();
        if msg == "Please sign in" {
            self.signed_out.set(true);
        }
        self.notify(msg, true);
    }

    // ---------- providers & models ----------

    pub fn load_providers(self) {
        spawn_local(async move {
            match api::get::<Vec<Provider>>("/providers").await {
                Ok(list) => {
                    self.providers.set(list);
                    self.providers_loaded.set(true);
                    self.ensure_model_selected();
                }
                Err(e) => self.error(e),
            }
        });
    }

    /// Picks the last used (or first) provider if the current one is missing.
    fn ensure_model_selected(self) {
        let providers = self.providers.get_untracked();
        let valid = self.provider_id.get_untracked().is_some_and(|id| providers.iter().any(|p| p.id == id));
        if valid {
            return;
        }
        let saved = theme::load_pref(LAST_MODEL_KEY).unwrap_or_default();
        let (pid, model) = saved.split_once('|').unwrap_or(("", ""));
        if let Some(p) = pid.parse::<i64>().ok().filter(|id| providers.iter().any(|p| p.id == *id)) {
            self.provider_id.set(Some(p));
            self.model.set(model.to_string());
        } else if let Some(first) = providers.first() {
            self.provider_id.set(Some(first.id));
            self.model.set(String::new());
            self.load_models(first.id, false);
        } else {
            self.provider_id.set(None);
            self.model.set(String::new());
        }
    }

    pub fn select_provider(self, pid: i64) {
        self.provider_id.set(Some(pid));
        self.model.set(String::new());
        self.load_models(pid, false);
    }

    pub fn select_model(self, model: String) {
        if let Some(pid) = self.provider_id.get_untracked() {
            theme::save_pref(LAST_MODEL_KEY, &format!("{pid}|{model}"));
        }
        self.model.set(model);
    }

    pub fn load_models(self, pid: i64, force: bool) {
        if !force && self.models.with_untracked(|m| m.contains_key(&pid)) {
            self.autofill_model(pid);
            return;
        }
        self.models_loading.set(true);
        spawn_local(async move {
            match api::get::<ModelList>(&format!("/providers/{pid}/models")).await {
                Ok(list) => {
                    let n = list.models.len();
                    self.models.update(|m| {
                        m.insert(pid, list.models);
                    });
                    self.autofill_model(pid);
                    if force {
                        self.notify(format!("Found {n} models"), false);
                    }
                }
                Err(e) => {
                    self.models.update(|m| {
                        m.insert(pid, Vec::new());
                    });
                    self.notify(format!("Couldn't list models: {e}. You can still type a model name."), true);
                }
            }
            self.models_loading.set(false);
        });
    }

    fn autofill_model(self, pid: i64) {
        if self.provider_id.get_untracked() != Some(pid) || !self.model.get_untracked().trim().is_empty() {
            return;
        }
        if let Some(first) = self.models.with_untracked(|m| m.get(&pid).and_then(|l| l.first().cloned())) {
            self.select_model(first);
        }
    }

    // ---------- conversations ----------

    pub fn load_conversations(self) {
        spawn_local(async move {
            match api::get::<Vec<ConversationSummary>>("/conversations").await {
                Ok(list) => {
                    // The server names a chat after its first message; keep the header in sync.
                    if let Some(id) = self.current_id.get_untracked() {
                        if let Some(c) = list.iter().find(|c| c.id == id) {
                            self.current_title.set(c.title.clone());
                        }
                    }
                    self.conversations.set(list);
                }
                Err(e) => self.error(e),
            }
        });
    }

    pub fn new_chat(self) {
        self.stop();
        self.current_id.set(None);
        self.current_title.set(String::new());
        self.messages.set(Vec::new());
        self.editing.set(None);
        self.sidebar_open.set(false);
    }

    pub fn open_conversation(self, id: i64) {
        self.sidebar_open.set(false);
        if self.current_id.get_untracked() == Some(id) {
            return;
        }
        self.stop();
        self.current_id.set(Some(id));
        self.editing.set(None);
        self.loading_chat.set(true);
        spawn_local(async move {
            let result = api::get::<Conversation>(&format!("/conversations/{id}")).await;
            if self.current_id.get_untracked() != Some(id) {
                return; // user switched again meanwhile
            }
            self.loading_chat.set(false);
            match result {
                Ok(conv) => {
                    self.current_title.set(conv.title);
                    self.messages.set(conv.messages.into_iter().map(MsgView::from_message).collect());
                    self.params.set(conv.params);
                    let known =
                        conv.provider_id.filter(|p| self.providers.with_untracked(|l| l.iter().any(|x| x.id == *p)));
                    if let (Some(pid), Some(model)) = (known, conv.model) {
                        self.provider_id.set(Some(pid));
                        self.model.set(model);
                        self.load_models(pid, false);
                    }
                    util::next_frame(util::highlight_code);
                }
                Err(e) => {
                    self.current_id.set(None);
                    self.messages.set(Vec::new());
                    self.error(e);
                }
            }
        });
    }

    pub fn rename_conversation(self, id: i64, current: String) {
        let Some(title) = util::prompt("Rename chat", &current) else { return };
        let title = title.trim().to_string();
        if title.is_empty() || title == current {
            return;
        }
        spawn_local(async move {
            let patch = ConversationPatch { title: Some(title.clone()), ..Default::default() };
            match api::send::<_, Conversation>("PATCH", &format!("/conversations/{id}"), &patch).await {
                Ok(_) => {
                    if self.current_id.get_untracked() == Some(id) {
                        self.current_title.set(title);
                    }
                    self.load_conversations();
                }
                Err(e) => self.error(e),
            }
        });
    }

    pub fn delete_conversation(self, id: i64) {
        if !util::confirm("Delete this chat? This can't be undone.") {
            return;
        }
        spawn_local(async move {
            match api::call("DELETE", &format!("/conversations/{id}")).await {
                Ok(()) => {
                    self.conversations.update(|l| l.retain(|c| c.id != id));
                    if self.current_id.get_untracked() == Some(id) {
                        self.new_chat();
                    }
                }
                Err(e) => self.error(e),
            }
        });
    }

    // ---------- sending ----------

    fn target(self) -> Option<(i64, String)> {
        let Some(pid) = self.provider_id.get_untracked() else {
            self.notify("Add an AI provider first (Settings → Providers)", true);
            self.settings.set(Some(SettingsTab::Providers));
            return None;
        };
        let model = self.model.get_untracked().trim().to_string();
        if model.is_empty() {
            self.notify("Pick or type a model name first", true);
            return None;
        }
        Some((pid, model))
    }

    pub fn send(self, content: String) {
        let content = content.trim().to_string();
        let files = self.pending.get_untracked();
        if files.iter().any(|f| f.att.is_none()) {
            self.notify("Wait for the files to finish uploading", true);
            return;
        }
        if (content.is_empty() && files.is_empty()) || self.streaming.get_untracked() {
            return;
        }
        let Some((provider_id, model)) = self.target() else { return };
        let replace_from = self.editing.get_untracked();
        let attachments: Vec<i64> = files.iter().filter_map(|f| f.att.as_ref().map(|a| a.id)).collect();
        self.streaming.set(true);
        self.draft.set(String::new());
        self.pending.set(Vec::new());
        spawn_local(async move {
            let conv_id = match self.current_id.get_untracked() {
                Some(id) => id,
                None => match api::send::<_, Conversation>("POST", "/conversations", &serde_json::json!({})).await {
                    Ok(conv) => {
                        self.current_id.set(Some(conv.id));
                        self.current_title.set(conv.title.clone());
                        self.conversations.update(|l| {
                            l.insert(
                                0,
                                ConversationSummary { id: conv.id, title: conv.title, updated_at: conv.updated_at },
                            )
                        });
                        conv.id
                    }
                    Err(e) => {
                        self.streaming.set(false);
                        self.draft.set(content);
                        self.pending.set(files);
                        return self.error(e);
                    }
                },
            };
            let body = SendMessage {
                content: content.clone(),
                provider_id,
                model,
                params: self.params.get_untracked(),
                replace_from,
                attachments,
                tools: self.tools.get_untracked(),
            };
            let started = self.run_stream(format!("/conversations/{conv_id}/messages"), body, replace_from).await;
            if started {
                self.editing.set(None);
            } else if self.draft.with_untracked(|d| d.is_empty()) {
                // Don't lose what the user typed or attached.
                self.draft.set(content);
                self.pending.set(files);
            }
        });
    }

    pub fn regenerate(self) {
        if self.streaming.get_untracked() {
            return;
        }
        let (Some(conv_id), Some((provider_id, model))) = (self.current_id.get_untracked(), self.target()) else {
            return;
        };
        self.streaming.set(true);
        spawn_local(async move {
            let body = Regenerate {
                provider_id,
                model,
                params: self.params.get_untracked(),
                tools: self.tools.get_untracked(),
            };
            self.run_stream(format!("/conversations/{conv_id}/regenerate"), body, None).await;
        });
    }

    /// Streams one answer. Returns whether the server accepted the request.
    async fn run_stream(self, path: String, body: impl Serialize, replace_from: Option<i64>) -> bool {
        let controller = web_sys::AbortController::new().expect("AbortController");
        let signal = controller.signal();
        self.abort.set_value(Some(controller));
        let conv_at_start = self.current_id.get_untracked();
        let assistant: Rc<Cell<Option<MsgView>>> = Rc::new(Cell::new(None));
        let started = Rc::new(Cell::new(false));

        let (a, s) = (assistant.clone(), started.clone());
        let result = api::stream(&path, &body, &signal, move |ev| {
            if self.current_id.get_untracked() != conv_at_start {
                return; // user navigated away; the server still saves the answer
            }
            match ev {
                StreamEvent::Start { user_message, assistant_message_id, model } => {
                    s.set(true);
                    let view = MsgView {
                        id: assistant_message_id,
                        role: MsgRole::Assistant,
                        content: RwSignal::new(String::new()),
                        model: RwSignal::new(Some(model)),
                        error: RwSignal::new(None),
                        streaming: RwSignal::new(true),
                        attachments: StoredValue::new(Vec::new()),
                        tool_calls: RwSignal::new(Vec::new()),
                    };
                    a.set(Some(view));
                    self.messages.update(|list| {
                        match (&user_message, replace_from) {
                            (Some(_), Some(from)) => list.retain(|m| m.id < from),
                            // Regenerate: drop old replies after the last user message.
                            (None, _) => {
                                if let Some(i) = list.iter().rposition(|m| m.role == MsgRole::User) {
                                    list.truncate(i + 1);
                                }
                            }
                            _ => {}
                        }
                        if let Some(um) = user_message {
                            list.push(MsgView::from_message(um));
                        }
                        list.push(view);
                    });
                }
                StreamEvent::Delta { text } => {
                    if let Some(v) = a.get() {
                        v.content.update(|c| c.push_str(&text));
                    }
                }
                StreamEvent::ToolStart { call } | StreamEvent::ToolEnd { call } => {
                    if let Some(v) = a.get() {
                        v.tool_calls.update(|list| match list.iter_mut().find(|c| c.id == call.id) {
                            Some(existing) => *existing = call,
                            None => list.push(call),
                        });
                    }
                }
                StreamEvent::Error { message } => {
                    if let Some(v) = a.get() {
                        v.error.set(Some(message.clone()));
                    }
                    self.notify(message, true);
                }
                StreamEvent::Done { message } => {
                    if let Some(v) = a.get() {
                        v.content.set(message.content);
                        v.error.set(message.error);
                        v.tool_calls.set(message.tool_calls);
                        v.streaming.set(false);
                    }
                }
            }
        })
        .await;

        if let Err(e) = result {
            match assistant.get() {
                Some(v) if e.is_abort() => v.error.set(Some("Stopped".into())),
                Some(v) => {
                    v.error.set(Some(e.message.clone()));
                    self.error(e);
                }
                None if e.is_abort() => {}
                None => self.error(e),
            }
        }
        if let Some(v) = assistant.get() {
            v.streaming.set(false);
        }
        self.abort.set_value(None);
        self.streaming.set(false);
        self.load_conversations();
        util::next_frame(util::highlight_code);
        started.get()
    }

    pub fn stop(self) {
        self.abort.with_value(|c| {
            if let Some(c) = c {
                c.abort();
            }
        });
    }

    pub fn start_edit(self, m: MsgView) {
        if self.streaming.get_untracked() {
            return;
        }
        self.editing.set(Some(m.id));
        self.draft.set(m.content.get_untracked());
        let files = m.attachments.get_value();
        self.pending.set(
            files
                .into_iter()
                .enumerate()
                .map(|(i, a)| PendingFile { key: i as u32, name: a.name.clone(), att: Some(a) })
                .collect(),
        );
    }

    pub fn cancel_edit(self) {
        self.editing.set(None);
        self.draft.set(String::new());
        self.pending.set(Vec::new());
    }

    // ---------- attachments & tools ----------

    pub fn add_files(self, files: Vec<web_sys::File>) {
        for file in files {
            let key = self.pending.with_untracked(|p| p.iter().map(|f| f.key).max().map_or(0, |k| k + 1));
            if self.pending.with_untracked(|p| p.len()) >= 10 {
                self.notify("At most 10 files per message", true);
                return;
            }
            let name = file.name();
            if file.size() > 20.0 * 1024.0 * 1024.0 {
                self.notify(format!("{name} is larger than 20 MB"), true);
                continue;
            }
            self.pending.update(|p| p.push(PendingFile { key, name: name.clone(), att: None }));
            spawn_local(async move {
                match api::upload(&file).await {
                    Ok(att) => self.pending.update(|p| {
                        if let Some(f) = p.iter_mut().find(|f| f.key == key) {
                            f.att = Some(att);
                        }
                    }),
                    Err(e) => {
                        self.pending.update(|p| p.retain(|f| f.key != key));
                        self.notify(format!("{name}: {e}"), true);
                    }
                }
            });
        }
    }

    pub fn remove_file(self, key: u32) {
        let removed = self.pending.with_untracked(|p| p.iter().find(|f| f.key == key).cloned());
        self.pending.update(|p| p.retain(|f| f.key != key));
        // Unsent uploads are deleted right away (files reused from an edited message stay).
        if let Some(att) = removed.and_then(|f| f.att) {
            if self.editing.get_untracked().is_none() {
                spawn_local(async move {
                    let _ = api::call("DELETE", &format!("/uploads/{}", att.id)).await;
                });
            }
        }
    }

    pub fn set_tools(self, f: impl FnOnce(&mut ToolSelection)) {
        self.tools.update(f);
        if let Ok(json) = serde_json::to_string(&self.tools.get_untracked()) {
            theme::save_pref(TOOLS_KEY, &json);
        }
    }

    pub fn load_mcp_servers(self) {
        spawn_local(async move {
            if let Ok(list) = api::get::<Vec<McpServer>>("/mcp").await {
                // Forget selections of servers that no longer exist.
                let ids: Vec<i64> = list.iter().map(|m| m.id).collect();
                if self.tools.with_untracked(|t| t.mcp_servers.iter().any(|id| !ids.contains(id))) {
                    self.set_tools(|t| t.mcp_servers.retain(|id| ids.contains(id)));
                }
                self.mcp_servers.set(list);
            }
        });
    }

    pub fn sign_out(self) {
        spawn_local(async move {
            let _ = api::call("POST", "/auth/logout").await;
            self.signed_out.set(true);
        });
    }
}
