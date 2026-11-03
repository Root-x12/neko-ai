//! Tools the model can call: built-in web tools and tools from the user's MCP servers.

pub mod mcp;
pub mod web;

use std::collections::HashMap;

use neko_shared::{McpTransport, Role, ToolSelection, User};
use serde_json::Value;

use crate::{db, llm::ToolDef, AppState};

pub const WEB_SEARCH: &str = "web_search";
pub const FETCH_URL: &str = "fetch_url";

enum Exec {
    WebSearch,
    FetchUrl,
    Mcp { config: Box<mcp::McpConfig>, tool: String },
}

struct Entry {
    exec: Exec,
    label: String,
}

/// The tools available to one generation.
pub struct Toolbox {
    pub defs: Vec<ToolDef>,
    entries: HashMap<String, Entry>,
    search: Option<web::SearchConfig>,
}

impl Toolbox {
    pub fn empty() -> Self {
        Self { defs: Vec::new(), entries: HashMap::new(), search: None }
    }

    /// Collects the selected tools. MCP servers that can't be reached are reported as warnings
    /// instead of failing the whole request.
    pub async fn build(s: &AppState, user: &User, sel: &ToolSelection) -> (Self, Vec<(String, String)>) {
        let mut tb = Self::empty();
        let mut warnings = Vec::new();
        let mut sel = sel.clone();
        if s.config.local_only && (sel.web_search || sel.fetch_url) {
            warnings.push(("Web tools".into(), "Web search and page reading are off in local-only mode".into()));
            sel.web_search = false;
            sel.fetch_url = false;
        }
        if sel.web_search {
            let uid = user.id;
            match s.db.call(move |c| db::search_settings_record(c, uid)).await {
                Ok(rec) => {
                    let api_key = rec.api_key_enc.and_then(|b| s.vault.decrypt(&b).ok());
                    tb.search = Some(web::SearchConfig {
                        backend: rec.settings.backend,
                        base_url: rec.settings.base_url,
                        api_key,
                    });
                    tb.add(
                        WEB_SEARCH,
                        "Web search",
                        "Search the web for current information. Returns titles, URLs and snippets; \
                         use fetch_url to read a result in full. Cite the URLs you rely on.",
                        web::web_search_schema(),
                        Exec::WebSearch,
                    );
                }
                Err(e) => warnings.push(("Web search".into(), e.to_string())),
            }
        }
        if sel.fetch_url {
            tb.add(
                FETCH_URL,
                "Read web page",
                "Fetch a web page, text file or PDF by URL and return its readable text.",
                web::fetch_url_schema(),
                Exec::FetchUrl,
            );
        }
        if !sel.mcp_servers.is_empty() {
            let (uid, ids) = (user.id, sel.mcp_servers.clone());
            let records = s.db.call(move |c| db::mcp_records(c, uid, &ids)).await.unwrap_or_default();
            for rec in records {
                let name = rec.server.name.clone();
                if !rec.server.enabled {
                    continue;
                }
                if rec.server.transport == McpTransport::Stdio && user.role != Role::Admin {
                    warnings.push((name, "Local-command MCP servers are admin-only".into()));
                    continue;
                }
                let config = match rec.decrypt(&s.vault) {
                    Ok(c) => c,
                    Err(e) => {
                        warnings.push((name, e));
                        continue;
                    }
                };
                if s.config.local_only && config.transport == McpTransport::Http {
                    if let Err(e) = web::ensure_local_url(&config.url).await {
                        warnings.push((name, e));
                        continue;
                    }
                }
                match s.mcp.list_tools(&s.http, &config).await {
                    Ok(tools) => {
                        for t in tools {
                            let alias = mcp::tool_alias(config.id, &t.name);
                            let desc = web::truncate(&format!("[{}] {}", config.name, t.description.trim()), 1000);
                            tb.add(
                                &alias,
                                &format!("{} › {}", config.name, t.name),
                                &desc,
                                mcp::clean_schema(t.input_schema),
                                Exec::Mcp { config: Box::new(config.clone()), tool: t.name },
                            );
                        }
                    }
                    Err(e) => warnings.push((name, e)),
                }
            }
        }
        (tb, warnings)
    }

    fn add(&mut self, name: &str, label: &str, description: &str, schema: Value, exec: Exec) {
        if self.entries.contains_key(name) {
            return;
        }
        self.defs.push(ToolDef { name: name.to_string(), description: description.to_string(), schema });
        self.entries.insert(name.to_string(), Entry { exec, label: label.to_string() });
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    pub fn label(&self, name: &str) -> String {
        self.entries.get(name).map(|e| e.label.clone()).unwrap_or_else(|| name.to_string())
    }

    /// Runs one tool call. Returns (text for the model, is_error).
    pub async fn run(&self, s: &AppState, name: &str, args: &Value) -> (String, bool) {
        let Some(entry) = self.entries.get(name) else {
            return (format!("Unknown tool `{name}`"), true);
        };
        let result = match &entry.exec {
            Exec::WebSearch => {
                let query = args["query"].as_str().unwrap_or_default();
                let count = args["count"].as_u64().unwrap_or(6) as usize;
                match &self.search {
                    Some(cfg) => {
                        web::web_search(&s.http, cfg, query, count).await.map(|r| web::format_results(query, &r))
                    }
                    None => Err("Web search is not configured".into()),
                }
            }
            Exec::FetchUrl => match args["url"].as_str() {
                Some(url) => web::fetch_url(url, s.config.fetch_allow_private).await,
                None => Err("Missing `url`".into()),
            },
            Exec::Mcp { config, tool } => {
                return s.mcp.call_tool(&s.http, config, tool, args.clone()).await.unwrap_or_else(|e| (e, true));
            }
        };
        match result {
            Ok(text) => (text, false),
            Err(e) => (e, true),
        }
    }
}
