//! Settings → Memory, Settings → My files, and the export menu.

use leptos::{prelude::*, task::spawn_local};
use neko_shared::*;
use wasm_bindgen::JsCast;

use crate::{
    api,
    icons::{self, Icon},
    markdown,
    state::Ctx,
    util,
};

// ---------- memory ----------

#[component]
pub fn MemoryTab() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let memories = RwSignal::new(Vec::<Memory>::new());
    let settings = RwSignal::new(None::<MemorySettings>);
    let draft = RwSignal::new(String::new());
    let editing = RwSignal::new(None::<(i64, String)>);
    let reload = move || {
        spawn_local(async move {
            match api::get::<Vec<Memory>>("/memory").await {
                Ok(list) => memories.set(list),
                Err(e) => ctx.error(e),
            }
        })
    };
    reload();
    spawn_local(async move {
        if let Ok(s) = api::get::<MemorySettings>("/memory/settings").await {
            settings.set(Some(s));
        }
    });
    let save_settings = move |next: MemorySettings| {
        settings.set(Some(next));
        spawn_local(async move {
            if let Err(e) = api::send::<_, MemorySettings>("PUT", "/memory/settings", &next).await {
                ctx.error(e);
            }
        });
    };
    let add = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let content = draft.get_untracked();
        if content.trim().is_empty() {
            return;
        }
        spawn_local(async move {
            match api::send::<_, Memory>("POST", "/memory", &MemoryInput { content }).await {
                Ok(_) => {
                    draft.set(String::new());
                    reload();
                }
                Err(e) => ctx.error(e),
            }
        });
    };
    let save_edit = move || {
        let Some((id, content)) = editing.get_untracked() else { return };
        spawn_local(async move {
            match api::send::<_, Memory>("PUT", &format!("/memory/{id}"), &MemoryInput { content }).await {
                Ok(_) => {
                    editing.set(None);
                    reload();
                }
                Err(e) => ctx.error(e),
            }
        });
    };
    let remove = move |id: i64| {
        spawn_local(async move {
            match api::call("DELETE", &format!("/memory/{id}")).await {
                Ok(()) => reload(),
                Err(e) => ctx.error(e),
            }
        });
    };
    let clear = move |_| {
        if !util::confirm("Forget everything in memory?") {
            return;
        }
        spawn_local(async move {
            match api::call("DELETE", "/memory").await {
                Ok(()) => reload(),
                Err(e) => ctx.error(e),
            }
        });
    };
    view! {
        <h3>"Memory"</h3>
        <p class="muted">
            "Short facts Neko keeps across chats, like your name, projects and how you like answers. They're added to every chat while memory is on. You can see and change all of it here."
        </p>
        {move || settings.get().map(|s| view! {
            <label class="check">
                <input type="checkbox" prop:checked=s.enabled on:change=move |ev| save_settings(MemorySettings { enabled: event_target_checked(&ev), ..s }) />
                "Use memory in chats"
            </label>
            <label class="check">
                <input
                    type="checkbox"
                    prop:checked=s.auto_save
                    disabled=!s.enabled
                    on:change=move |ev| save_settings(MemorySettings { auto_save: event_target_checked(&ev), ..s })
                />
                "Let the AI save memories by itself"
                <span class="muted small">"(needs a model with tool support)"</span>
            </label>
        })}
        <form class="inline-form" on:submit=add>
            <input type="text" maxlength="500" placeholder="e.g. I prefer short answers with code examples" prop:value=move || draft.get() on:input=move |ev| draft.set(event_target_value(&ev)) />
            <button class="btn primary" type="submit"><Icon svg=icons::PLUS size=15 />"Remember"</button>
        </form>
        <div class="memory-list">
            <Show when=move || memories.with(|m| m.is_empty())>
                <p class="muted">"Nothing remembered yet."</p>
            </Show>
            <For each=move || memories.get() key=|m| (m.id, m.content.clone()) let:m>
                {
                    let id = m.id;
                    let text = m.content.clone();
                    view! {
                        <div class="memory-row">
                            <Icon svg=icons::BRAIN size=16 />
                            <Show
                                when=move || editing.with(|e| e.as_ref().is_some_and(|(eid, _)| *eid == id))
                                fallback=move || view! { <span class="memory-text">{m.content.clone()}</span> }
                            >
                                <input
                                    class="memory-edit"
                                    type="text"
                                    maxlength="500"
                                    prop:value=move || editing.with(|e| e.as_ref().map(|(_, t)| t.clone()).unwrap_or_default())
                                    on:input=move |ev| editing.set(Some((id, event_target_value(&ev))))
                                    on:keydown=move |ev| {
                                        if ev.key() == "Enter" {
                                            ev.prevent_default();
                                            save_edit();
                                        } else if ev.key() == "Escape" {
                                            editing.set(None);
                                        }
                                    }
                                />
                                <button class="btn small primary" on:click=move |_| save_edit()>"Save"</button>
                            </Show>
                            <span class="muted small">{util::ago(m.updated_at)}</span>
                            <button class="icon-btn small" aria-label="Edit memory" on:click=move |_| editing.set(Some((id, text.clone())))>
                                <Icon svg=icons::EDIT size=14 />
                            </button>
                            <button class="icon-btn small danger" aria-label="Forget" on:click=move |_| remove(id)>
                                <Icon svg=icons::TRASH size=14 />
                            </button>
                        </div>
                    }
                }
            </For>
        </div>
        <Show when=move || memories.with(|m| !m.is_empty())>
            <button class="btn danger-outline" on:click=clear>"Forget everything"</button>
        </Show>
    }
}

// ---------- knowledge ----------

#[component]
pub fn KnowledgeTab() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let docs = RwSignal::new(Vec::<Document>::new());
    let uploading = RwSignal::new(0usize);
    let query = RwSignal::new(String::new());
    let hits = RwSignal::new(None::<Vec<KnowledgeHit>>);
    let input = NodeRef::<leptos::html::Input>::new();
    let reload = move || {
        spawn_local(async move {
            match api::get::<Vec<Document>>("/knowledge").await {
                Ok(list) => docs.set(list),
                Err(e) => ctx.error(e),
            }
        })
    };
    reload();
    let upload = move |files: Vec<web_sys::File>| {
        for file in files {
            uploading.update(|n| *n += 1);
            spawn_local(async move {
                match api::upload_to::<Document>("/knowledge", &file).await {
                    Ok(d) => ctx.notify(format!("Indexed {} ({} parts)", d.name, d.chunks), false),
                    Err(e) => ctx.notify(format!("{}: {e}", file.name()), true),
                }
                uploading.update(|n| *n -= 1);
                reload();
            });
        }
    };
    let remove = move |d: Document| {
        if !util::confirm(&format!("Remove {} from your files?", d.name)) {
            return;
        }
        spawn_local(async move {
            match api::call("DELETE", &format!("/knowledge/{}", d.id)).await {
                Ok(()) => reload(),
                Err(e) => ctx.error(e),
            }
        });
    };
    let search = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let q = query.get_untracked();
        spawn_local(async move {
            let path = format!("/knowledge/search?q={}", String::from(js_sys::encode_uri_component(&q)));
            match api::get::<Vec<KnowledgeHit>>(&path).await {
                Ok(list) => hits.set(Some(list)),
                Err(e) => ctx.error(e),
            }
        });
    };
    view! {
        <h3>"My files"</h3>
        <p class="muted">
            "Add notes, docs, PDFs and code. Turn on " <strong>"Search my files"</strong>
            " in the wrench menu and the AI looks up the relevant passages. Search runs on your server (keyword ranking, no external service), and secrets inside your files are redacted before passages are sent."
        </p>
        <input
            node_ref=input
            type="file"
            multiple
            hidden
            accept="application/pdf,text/*,.md,.csv,.json,.rs,.py,.js,.ts,.go,.java,.c,.cpp,.h,.toml,.yaml,.yml,.sh,.sql,.html,.css,.xml,.log"
            on:change=move |_| {
                if let Some(el) = input.get_untracked() {
                    if let Some(list) = el.files() {
                        upload((0..list.length()).filter_map(|i| list.get(i)).collect());
                    }
                    el.set_value("");
                }
            }
        />
        <div
            class="dropzone"
            on:click=move |_| {
                if let Some(el) = input.get_untracked() {
                    el.click();
                }
            }
            on:dragover=move |ev: web_sys::DragEvent| ev.prevent_default()
            on:drop=move |ev: web_sys::DragEvent| {
                ev.prevent_default();
                if let Some(list) = ev.data_transfer().and_then(|d| d.files()) {
                    upload((0..list.length()).filter_map(|i| list.get(i)).collect());
                }
            }
        >
            <Icon svg=icons::UPLOAD size=22 />
            <span>{move || if uploading.get() > 0 { format!("Indexing {} file(s)…", uploading.get()) } else { "Click or drop files here".into() }}</span>
            <span class="muted small">"PDF, Markdown, text and code · up to 20 MB each"</span>
        </div>
        <div class="doc-list">
            <For each=move || docs.get() key=|d| d.id let:d>
                {
                    let dd = d.clone();
                    view! {
                        <div class="doc-row">
                            <Icon svg=if d.mime == "application/pdf" { icons::PDF } else { icons::FILE } size=18 />
                            <div class="doc-info">
                                <strong>{d.name.clone()}</strong>
                                <span class="muted small">
                                    {util::file_size(d.size)} " · " {d.chunks} " parts · added " {util::ago(d.created_at)}
                                </span>
                            </div>
                            {(!d.flags.is_empty()).then(|| view! {
                                <span class="flag-badge" title=d.flags.join("; ")>"⚠ injection?"</span>
                            })}
                            <button class="icon-btn small danger" aria-label="Remove file" on:click=move |_| remove(dd.clone())>
                                <Icon svg=icons::TRASH size=14 />
                            </button>
                        </div>
                    }
                }
            </For>
        </div>
        <Show when=move || docs.with(|d| !d.is_empty())>
            <form class="inline-form" on:submit=search>
                <input type="search" placeholder="Try a search, e.g. vacation policy" prop:value=move || query.get() on:input=move |ev| query.set(event_target_value(&ev)) />
                <button class="btn ghost" type="submit"><Icon svg=icons::SEARCH size=15 />"Search"</button>
            </form>
            {move || hits.get().map(|list| if list.is_empty() {
                view! { <p class="muted">"No matching passages."</p> }.into_any()
            } else {
                view! {
                    <div class="hit-list">
                        {list.into_iter().map(|h| view! {
                            <div class="hit">
                                <span class="muted small">{h.document} " · part " {h.chunk + 1}</span>
                                <p>{util::excerpt(&h.text, 320)}</p>
                            </div>
                        }).collect_view()}
                    </div>
                }.into_any()
            })}
        </Show>
    }
}

// ---------- export ----------

/// Builds a print-only copy of the chat and opens the browser's print dialog ("Save as PDF").
pub fn print_chat(title: &str, messages: &[(MsgRole, Option<String>, String)]) {
    let Some(doc) = util::window().document() else { return };
    let Some(body) = doc.body() else { return };
    if let Some(old) = doc.get_element_by_id("print-root") {
        old.remove();
    }
    let mut html = format!(
        "<header class=\"print-head\"><h1>{}</h1><p>Exported from Neko AI · {}</p></header>",
        markdown::escape(title),
        String::from(js_sys::Date::new_0().to_locale_string("en-GB", &wasm_bindgen::JsValue::UNDEFINED))
    );
    for (role, model, content) in messages {
        let who = match role {
            MsgRole::User => "You".to_string(),
            MsgRole::Assistant => model.clone().unwrap_or_else(|| "Assistant".into()),
        };
        let class = if *role == MsgRole::User { "user" } else { "assistant" };
        html.push_str(&format!(
            "<section class=\"print-msg {class}\"><div class=\"print-who\">{}</div><div class=\"md\">{}</div></section>",
            markdown::escape(&who),
            markdown::render(content)
        ));
    }
    let Ok(root) = doc.create_element("div") else { return };
    root.set_id("print-root");
    root.set_inner_html(&html);
    let _ = body.append_child(&root);
    util::highlight_code();
    let _ = doc.document_element().map(|e| e.class_list().add_1("printing"));
    // Give highlight.js a frame, then print; clean up once the dialog closes.
    util::next_frame(move || {
        let w = util::window();
        let cleanup = wasm_bindgen::closure::Closure::once_into_js(move || {
            if let Some(d) = util::window().document() {
                if let Some(r) = d.get_element_by_id("print-root") {
                    r.remove();
                }
                let _ = d.document_element().map(|e| e.class_list().remove_1("printing"));
            }
        });
        let _ = w.add_event_listener_with_callback("afterprint", cleanup.unchecked_ref());
        let _ = w.print();
    });
}

#[component]
pub fn ExportMenu() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let open = RwSignal::new(false);
    let fetch_file = move |format: &'static str| {
        open.set(false);
        let Some(id) = ctx.current_id.get_untracked() else { return };
        // Same-origin link: the session cookie authorises the download.
        if let Some(a) = util::window().document().and_then(|d| d.create_element("a").ok()) {
            let a: web_sys::HtmlAnchorElement = a.unchecked_into();
            a.set_href(&format!("/api/conversations/{id}/export?format={format}"));
            a.set_download("");
            a.click();
        }
    };
    let print = move |_| {
        open.set(false);
        let msgs: Vec<(MsgRole, Option<String>, String)> = ctx.messages.with_untracked(|l| {
            l.iter().map(|m| (m.role, m.model.get_untracked(), m.content.get_untracked())).collect()
        });
        print_chat(&ctx.current_title.get_untracked(), &msgs);
    };
    view! {
        <Show when=move || ctx.current_id.get().is_some()>
            <div class="tools-menu export-menu">
                <button class="icon-btn" title="Export chat" aria-label="Export chat" on:click=move |_| open.update(|o| *o = !*o)>
                    <Icon svg=icons::DOWNLOAD />
                </button>
                <Show when=move || open.get()>
                    <div class="popover-scrim" on:click=move |_| open.set(false)></div>
                    <div class="popover down" role="menu">
                        <p class="popover-title">"Export this chat"</p>
                        <button class="menu-item" on:click=move |_| fetch_file("markdown")><Icon svg=icons::FILE size=15 />"Markdown (.md)"</button>
                        <button class="menu-item" on:click=print><Icon svg=icons::PDF size=15 />"PDF (print / save as PDF)"</button>
                        <button class="menu-item" on:click=move |_| fetch_file("json")><Icon svg=icons::CODE_BRACES size=15 />"JSON (full data)"</button>
                    </div>
                </Show>
            </div>
        </Show>
    }
}
