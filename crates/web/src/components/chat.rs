use leptos::{html, prelude::*};
use neko_shared::{Attachment, AttachmentKind, MsgRole, ToolCall};
use wasm_bindgen::JsCast;

use crate::{
    components::{mascot::Mascot, ThemeToggles},
    icons::{self, Icon},
    markdown,
    state::{Ctx, MsgView, SettingsTab},
    theme::Mode,
    util,
};

#[component]
pub fn ChatView() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let scroller = NodeRef::<html::Div>::new();
    let pinned = RwSignal::new(true);

    // Follow the stream while the user is at the bottom; leave them alone if they scrolled up.
    Effect::new(move |_| {
        ctx.messages.with(|m| {
            if let Some(last) = m.last() {
                last.content.track();
            }
        });
        if pinned.get_untracked() {
            util::next_frame(move || {
                if let Some(el) = scroller.get_untracked() {
                    el.set_scroll_top(el.scroll_height());
                }
            });
        }
    });
    // Jump to the bottom when switching chats.
    Effect::new(move |_| {
        ctx.current_id.track();
        pinned.set(true);
    });

    let on_scroll = move |_| {
        if let Some(el) = scroller.get_untracked() {
            pinned.set(el.scroll_height() - el.scroll_top() - el.client_height() < 80);
        }
    };

    // Copy buttons inside rendered markdown, and external links opening in a new tab.
    let on_click = move |ev: leptos::ev::MouseEvent| {
        let Some(target) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
        if let Ok(Some(btn)) = target.closest("[data-copy-code]") {
            let code = btn
                .closest(".code-block")
                .ok()
                .flatten()
                .and_then(|b| b.query_selector("code").ok().flatten())
                .and_then(|c| c.text_content())
                .unwrap_or_default();
            util::copy_text(&code);
            btn.set_text_content(Some("Copied!"));
            set_timeout(move || btn.set_text_content(Some("Copy")), std::time::Duration::from_millis(1500));
            return;
        }
        if let Ok(Some(btn)) = target.closest("[data-preview]") {
            let index = btn.get_attribute("data-preview").and_then(|v| v.parse::<usize>().ok());
            let msg = btn.closest("article[data-msg-id]").ok().flatten().and_then(|a| a.get_attribute("data-msg-id"));
            if let (Some(index), Some(msg_id)) = (index, msg.and_then(|v| v.parse::<i64>().ok())) {
                ctx.open_artifact(msg_id, index);
            }
            return;
        }
        if let Ok(Some(a)) = target.closest(".md a[href]") {
            let href = a.get_attribute("href").unwrap_or_default();
            if href.starts_with("http://") || href.starts_with("https://") {
                ev.prevent_default();
                let _ = util::window().open_with_url_and_target_and_features(&href, "_blank", "noopener,noreferrer");
            }
        }
    };

    view! {
        <main class="chat">
            <TopBar />
            <div class="messages" node_ref=scroller on:scroll=on_scroll on:click=on_click>
                <Show
                    when=move || !ctx.messages.with(|m| m.is_empty()) || ctx.loading_chat.get()
                    fallback=move || view! { <EmptyState /> }
                >
                    <div class="msg-list">
                        <For each=move || ctx.messages.get() key=|m| m.id let:m>
                            <MessageItem m=m />
                        </For>
                    </div>
                </Show>
            </div>
            <Composer />
        </main>
    }
}

#[component]
fn TopBar() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let title = move || {
        let t = ctx.current_title.get();
        if t.is_empty() {
            "New chat".to_string()
        } else {
            t
        }
    };
    view! {
        <header class="topbar">
            <button class="icon-btn only-mobile" aria-label="Open menu" on:click=move |_| ctx.sidebar_open.set(true)>
                <Icon svg=icons::MENU />
            </button>
            <h2
                class="chat-title"
                class:clickable=move || ctx.current_id.get().is_some()
                title="Rename"
                on:click=move |_| {
                    if let Some(id) = ctx.current_id.get_untracked() {
                        ctx.rename_conversation(id, ctx.current_title.get_untracked());
                    }
                }
            >
                {title}
            </h2>
            <div class="topbar-actions">
                <ThemeToggles />
                <button
                    class="icon-btn"
                    class:active=move || ctx.params_open.get()
                    aria-label="Chat settings"
                    title="System prompt & parameters"
                    on:click=move |_| ctx.params_open.update(|o| *o = !*o)
                >
                    <Icon svg=icons::SLIDERS />
                </button>
            </div>
        </header>
    }
}

#[component]
fn EmptyState() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let cyber = move || ctx.theme.mode.get() == Mode::Cyber;
    let suggestions = move || -> &'static [&'static str] {
        if cyber() {
            &[
                "Review this Rust function for bugs",
                "Explain async/await like I'm a senior dev",
                "Write a bash script that backs up a folder",
                "Design a REST API for a todo app",
            ]
        } else {
            &[
                "Tell me a cozy bedtime story about a cat",
                "Plan a slow, relaxing Sunday",
                "Explain black holes simply",
                "Write a haiku about rainy days",
            ]
        }
    };
    view! {
        <div class="empty">
            <Mascot state=Signal::derive(move || if ctx.idle.get() { "sleep" } else { "idle" }) big=true />
            <h1 class="empty-title">
                {move || if cyber() { "> ready_to_code" } else { "Purr… how can I help?" }}
            </h1>
            <p class="muted">
                {move || if ctx.providers_loaded.get() && ctx.providers.with(|p| p.is_empty()) {
                    "Add an API key from any provider to start chatting."
                } else if cyber() {
                    "Cyber mode: sharp focus for coding sessions."
                } else {
                    "Neko mode: cozy vibes for thinking and relaxing."
                }}
            </p>
            <Show
                when=move || !(ctx.providers_loaded.get() && ctx.providers.with(|p| p.is_empty()))
                fallback=move || view! {
                    <button class="btn primary" on:click=move |_| ctx.settings.set(Some(SettingsTab::Providers))>
                        <Icon svg=icons::KEY />
                        "Add a provider"
                    </button>
                }
            >
                <div class="suggestions">
                    {move || suggestions()
                        .iter()
                        .map(|s| {
                            let s = *s;
                            view! { <button class="suggestion" on:click=move |_| ctx.draft.set(s.to_string())>{s}</button> }
                        })
                        .collect_view()}
                </div>
            </Show>
        </div>
    }
}

#[component]
fn MessageItem(m: MsgView) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let is_user = m.role == MsgRole::User;
    let html = Memo::new(move |_| markdown::render(&m.content.get()));
    // Re-highlight when a finished message is (re)rendered.
    Effect::new(move |_| {
        html.track();
        if !m.streaming.get() {
            util::next_frame(util::highlight_code);
        }
    });
    let is_last_assistant = move || !is_user && ctx.messages.with(|l| l.last().is_some_and(|x| x.id == m.id));
    let copy = move |_| {
        util::copy_text(&m.content.get_untracked());
        ctx.notify("Copied", false);
    };
    let files = m.attachments.get_value();

    view! {
        <article
            class="msg"
            data-msg-id=m.id
            class:user=is_user
            class:assistant=!is_user
            class:streaming=move || m.streaming.get()
            class:editing=move || ctx.editing.get() == Some(m.id)
        >
            <Show when=move || !is_user>
                <div class="msg-meta">
                    <span class="msg-avatar">
                        <Icon svg=icons::PAW size=14 />
                    </span>
                    <span class="msg-model">{move || m.model.get().unwrap_or_else(|| "assistant".into())}</span>
                </div>
            </Show>
            {(!files.is_empty()).then(|| view! { <AttachmentList files=files.clone() /> })}
            <Show when=move || m.tool_calls.with(|c| !c.is_empty())>
                <div class="tool-list">
                    <For each=move || m.tool_calls.get() key=|c| (c.id.clone(), c.done) let:call>
                        <ToolCard call=call />
                    </For>
                </div>
            </Show>
            <Show when=move || is_user || !m.content.with(|c| c.is_empty()) || m.streaming.get() || m.error.with(|e| e.is_some())>
                <div class="msg-body">
                    <Show when=move || m.streaming.get() && m.content.with(|c| c.is_empty())>
                        <div class="typing" aria-label="Thinking">
                            <span></span>
                            <span></span>
                            <span></span>
                        </div>
                    </Show>
                    <div class="md" inner_html=move || html.get()></div>
                    {move || {
                        m.error
                            .get()
                            .map(|e| {
                                let stopped = e == "Stopped";
                                view! {
                                    <div class="msg-error" class:soft=stopped>
                                        {if stopped { "⏹ Stopped".to_string() } else { format!("⚠ {e}") }}
                                    </div>
                                }
                            })
                    }}
                </div>
            </Show>
            <Show when=move || !m.streaming.get()>
                <div class="msg-actions">
                    <button class="icon-btn small" title="Copy" aria-label="Copy message" on:click=copy>
                        <Icon svg=icons::COPY size=15 />
                    </button>
                    <Show when=move || is_user>
                        <button
                            class="icon-btn small"
                            title="Edit & resend"
                            aria-label="Edit message"
                            disabled=move || ctx.streaming.get()
                            on:click=move |_| ctx.start_edit(m)
                        >
                            <Icon svg=icons::EDIT size=15 />
                        </button>
                    </Show>
                    <Show when=is_last_assistant>
                        <button
                            class="icon-btn small"
                            title="Regenerate with the selected model"
                            aria-label="Regenerate"
                            disabled=move || ctx.streaming.get()
                            on:click=move |_| ctx.regenerate()
                        >
                            <Icon svg=icons::REFRESH size=15 />
                        </button>
                    </Show>
                </div>
            </Show>
        </article>
    }
}

/// Thumbnails for images, chips for other files. Images open full size in a new tab.
#[component]
fn AttachmentList(files: Vec<Attachment>) -> impl IntoView {
    view! {
        <div class="attachments">
            {files
                .into_iter()
                .map(|f| {
                    let url = format!("/api/uploads/{}", f.id);
                    let (href, title) = (url.clone(), f.name.clone());
                    if f.kind == AttachmentKind::Image {
                        view! {
                            <a class="thumb" href=href target="_blank" rel="noopener" title=title>
                                <img src=url alt=f.name loading="lazy" />
                            </a>
                        }
                            .into_any()
                    } else {
                        view! {
                            <a class="file-chip" href=url download=title.clone() title=title>
                                <Icon svg=if f.kind == AttachmentKind::Pdf { icons::PDF } else { icons::FILE } size=15 />
                                <span>{f.name}</span>
                                <span class="muted small">{util::file_size(f.size)}</span>
                            </a>
                        }
                            .into_any()
                    }
                })
                .collect_view()}
        </div>
    }
}

/// One tool the model used: what it asked for, and (expandable) what came back.
#[component]
fn ToolCard(call: ToolCall) -> impl IntoView {
    let args: serde_json::Value = serde_json::from_str(&call.arguments).unwrap_or_default();
    let headline = match call.name.as_str() {
        "web_search" => args["query"].as_str().map(|q| format!("“{q}”")),
        "fetch_url" => args["url"].as_str().map(str::to_string),
        _ => args.as_object().and_then(|o| o.values().find_map(|v| v.as_str().map(|s| s.to_string()))),
    }
    .unwrap_or_default();
    let icon = match call.name.as_str() {
        "web_search" => icons::SEARCH,
        "fetch_url" => icons::GLOBE,
        _ => icons::WRENCH,
    };
    let status = if !call.done {
        "running"
    } else if call.is_error {
        "error"
    } else {
        "ok"
    };
    let pretty_args = serde_json::to_string_pretty(&args).unwrap_or_else(|_| call.arguments.clone());
    let has_args = !call.name.is_empty();
    view! {
        <details class="tool-card" data-status=status>
            <summary>
                <span class="tool-icon">
                    <Icon svg=icon size=15 />
                </span>
                <span class="tool-label">{call.label}</span>
                <span class="tool-headline">{headline}</span>
                <span class="tool-status">
                    {match status {
                        "running" => view! { <span class="spinner"></span> }.into_any(),
                        "error" => view! { <span>"⚠"</span> }.into_any(),
                        _ => view! { <span>"✓"</span> }.into_any(),
                    }}
                </span>
            </summary>
            {has_args.then(|| view! { <pre class="tool-pre">{pretty_args}</pre> })}
            {(!call.result.is_empty()).then(|| view! { <pre class="tool-pre result">{call.result}</pre> })}
        </details>
    }
}

/// Provider dropdown + searchable model combobox. Changing either mid-chat applies to the next message.
#[component]
fn ModelPicker() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let open = RwSignal::new(false);
    let filter = RwSignal::new(String::new());

    let models = move || {
        let pid = ctx.provider_id.get();
        let f = filter.get().to_lowercase();
        ctx.models.with(|m| {
            pid.and_then(|p| m.get(&p))
                .map(|l| l.iter().filter(|x| x.to_lowercase().contains(&f)).take(200).cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        })
    };

    view! {
        <div class="model-picker">
            <select
                class="provider-select"
                aria-label="Provider"
                on:change=move |ev| {
                    match event_target_value(&ev).parse::<i64>() {
                        Ok(id) => ctx.select_provider(id),
                        Err(_) => ctx.settings.set(Some(SettingsTab::Providers)),
                    }
                }
                prop:value=move || ctx.provider_id.get().map(|p| p.to_string()).unwrap_or_default()
            >
                <option value="" disabled>"Provider…"</option>
                <For each=move || ctx.providers.get() key=|p| (p.id, p.name.clone()) let:p>
                    <option value=p.id.to_string()>{p.name}</option>
                </For>
                <option value="add">"+ Add provider…"</option>
            </select>
            <div class="combo">
                <input
                    class="model-input"
                    type="text"
                    placeholder="model name"
                    aria-label="Model"
                    spellcheck="false"
                    autocapitalize="none"
                    prop:value=move || ctx.model.get()
                    on:focus=move |_| {
                        filter.set(String::new());
                        open.set(true);
                        if let Some(pid) = ctx.provider_id.get_untracked() {
                            ctx.load_models(pid, false);
                        }
                    }
                    on:blur=move |_| open.set(false)
                    on:input=move |ev| {
                        let v = event_target_value(&ev);
                        filter.set(v.clone());
                        ctx.select_model(v);
                        open.set(true);
                    }
                    on:keydown=move |ev| {
                        if ev.key() == "Escape" || ev.key() == "Enter" {
                            open.set(false);
                        }
                    }
                />
                <Show when=move || open.get() && !models().is_empty()>
                    <ul class="combo-list" role="listbox">
                        {move || models()
                            .into_iter()
                            .map(|name| {
                                let n = name.clone();
                                view! {
                                    <li
                                        role="option"
                                        class:selected=move || ctx.model.get() == n
                                        // mousedown fires before the input's blur
                                        on:mousedown=move |ev| {
                                            ev.prevent_default();
                                            ctx.select_model(name.clone());
                                            open.set(false);
                                        }
                                    >
                                        {name.clone()}
                                    </li>
                                }
                            })
                            .collect_view()}
                    </ul>
                </Show>
            </div>
            <button
                class="icon-btn small"
                class:spin=move || ctx.models_loading.get()
                title="Refresh model list"
                aria-label="Refresh model list"
                disabled=move || ctx.provider_id.get().is_none()
                on:click=move |_| {
                    if let Some(pid) = ctx.provider_id.get_untracked() {
                        ctx.load_models(pid, true);
                    }
                }
            >
                <Icon svg=icons::REFRESH size=15 />
            </button>
        </div>
    }
}

#[component]
fn ToolsMenu() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let open = RwSignal::new(false);
    let count = move || ctx.tools.with(|t| t.web_search as usize + t.fetch_url as usize + t.mcp_servers.len());
    let servers = move || ctx.mcp_servers.get().into_iter().filter(|m| m.enabled).collect::<Vec<_>>();
    view! {
        <div class="tools-menu">
            <button
                type="button"
                class="icon-btn small"
                class:active=move || { count() > 0 }
                title="Tools"
                aria-label="Tools"
                aria-expanded=move || open.get().to_string()
                on:click=move |_| open.update(|o| *o = !*o)
            >
                <Icon svg=icons::WRENCH size=16 />
                <Show when=move || { count() > 0 }>
                    <span class="badge">{count}</span>
                </Show>
            </button>
            <Show when=move || open.get()>
                <div class="popover-scrim" on:click=move |_| open.set(false)></div>
                <div class="popover" role="menu">
                    <p class="popover-title">"Let the AI use"</p>
                    <label class="check">
                        <input
                            type="checkbox"
                            prop:checked=move || ctx.tools.with(|t| t.web_search)
                            on:change=move |ev| {
                                let on = event_target_checked(&ev);
                                ctx.set_tools(|t| t.web_search = on);
                            }
                        />
                        <Icon svg=icons::SEARCH size=15 />
                        "Web search"
                    </label>
                    <label class="check">
                        <input
                            type="checkbox"
                            prop:checked=move || ctx.tools.with(|t| t.fetch_url)
                            on:change=move |ev| {
                                let on = event_target_checked(&ev);
                                ctx.set_tools(|t| t.fetch_url = on);
                            }
                        />
                        <Icon svg=icons::GLOBE size=15 />
                        "Read web pages"
                    </label>
                    <For
                        each=servers
                        key=|m| (m.id, m.name.clone())
                        let:m
                    >
                        {
                            let id = m.id;
                            view! {
                                <label class="check">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || ctx.tools.with(|t| t.mcp_servers.contains(&id))
                                        on:change=move |ev| {
                                            let on = event_target_checked(&ev);
                                            ctx.set_tools(|t| {
                                                t.mcp_servers.retain(|x| *x != id);
                                                if on {
                                                    t.mcp_servers.push(id);
                                                }
                                            });
                                        }
                                    />
                                    <Icon svg=icons::WRENCH size=15 />
                                    {m.name}
                                </label>
                            }
                        }
                    </For>
                    <button
                        class="btn small ghost wide"
                        on:click=move |_| {
                            open.set(false);
                            ctx.settings.set(Some(SettingsTab::Tools));
                        }
                    >
                        "Configure tools & MCP…"
                    </button>
                </div>
            </Show>
        </div>
    }
}

fn files_from(list: Option<web_sys::FileList>) -> Vec<web_sys::File> {
    let Some(list) = list else { return Vec::new() };
    (0..list.length()).filter_map(|i| list.get(i)).collect()
}

#[component]
fn Composer() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let input = NodeRef::<html::Textarea>::new();
    let file_input = NodeRef::<html::Input>::new();
    let dragging = RwSignal::new(false);

    // Auto-grow the textarea with its content (also when the draft is set programmatically).
    Effect::new(move |_| {
        ctx.draft.track();
        util::next_frame(move || {
            if let Some(el) = input.get_untracked() {
                let style = web_sys::HtmlElement::style(&el);
                let _ = style.set_property("height", "auto");
                let h = el.scroll_height().min(260);
                let _ = style.set_property("height", &format!("{h}px"));
            }
        });
    });
    // Focus the box when editing starts.
    Effect::new(move |prev: Option<Option<i64>>| {
        let editing = ctx.editing.get();
        if prev.is_some() && editing.is_some() {
            if let Some(el) = input.get_untracked() {
                let _ = el.focus();
            }
        }
        editing
    });

    let submit = move || ctx.send(ctx.draft.get_untracked());
    let can_send = move || {
        ctx.pending.with(|p| p.iter().all(|f| f.att.is_some()))
            && (ctx.draft.with(|d| !d.trim().is_empty()) || ctx.pending.with(|p| !p.is_empty()))
    };

    view! {
        <div class="composer-wrap">
            <Show when=move || ctx.editing.get().is_some()>
                <div class="edit-banner">
                    <Icon svg=icons::EDIT size=14 />
                    "Editing — sending replaces this message and everything after it"
                    <button class="icon-btn small" aria-label="Cancel edit" on:click=move |_| ctx.cancel_edit()>
                        <Icon svg=icons::X size=14 />
                    </button>
                </div>
            </Show>
            <form
                class="composer"
                class:dragging=move || dragging.get()
                on:submit=move |ev| {
                    ev.prevent_default();
                    submit();
                }
                on:dragover=move |ev: web_sys::DragEvent| {
                    ev.prevent_default();
                    dragging.set(true);
                }
                on:dragleave=move |_| dragging.set(false)
                on:drop=move |ev: web_sys::DragEvent| {
                    ev.prevent_default();
                    dragging.set(false);
                    ctx.add_files(files_from(ev.data_transfer().and_then(|d| d.files())));
                }
            >
                <Show when=move || ctx.pending.with(|p| !p.is_empty())>
                    <div class="pending-files">
                        <For each=move || ctx.pending.get() key=|f| (f.key, f.att.is_some()) let:f>
                            {
                                let key = f.key;
                                let thumb = f
                                    .att
                                    .as_ref()
                                    .filter(|a| a.kind == AttachmentKind::Image)
                                    .map(|a| format!("/api/uploads/{}", a.id));
                                view! {
                                    <div class="pending-file" class:uploading=f.att.is_none()>
                                        {match thumb {
                                            Some(src) => view! { <img src=src alt="" /> }.into_any(),
                                            None => {
                                                view! {
                                                    <span class="pending-icon">
                                                        {if f.att.is_none() {
                                                            view! { <span class="spinner"></span> }.into_any()
                                                        } else {
                                                            view! { <Icon svg=icons::FILE size=16 /> }.into_any()
                                                        }}
                                                    </span>
                                                }
                                                    .into_any()
                                            }
                                        }}
                                        <span class="pending-name">{f.name.clone()}</span>
                                        <button
                                            type="button"
                                            class="icon-btn small"
                                            aria-label="Remove file"
                                            on:click=move |_| ctx.remove_file(key)
                                        >
                                            <Icon svg=icons::X size=13 />
                                        </button>
                                    </div>
                                }
                            }
                        </For>
                    </div>
                </Show>
                <textarea
                    node_ref=input
                    rows="1"
                    placeholder=move || {
                        if ctx.theme.mode.get() == Mode::Cyber { "$ ask anything…" } else { "Message Neko…" }
                    }
                    aria-label="Message"
                    prop:value=move || ctx.draft.get()
                    on:input=move |ev| ctx.draft.set(event_target_value(&ev))
                    on:paste=move |ev: web_sys::ClipboardEvent| {
                        let files = files_from(ev.clipboard_data().and_then(|d| d.files()));
                        if !files.is_empty() {
                            ev.prevent_default();
                            ctx.add_files(files);
                        }
                    }
                    on:keydown=move |ev| {
                        // Enter sends on desktop; on phones Enter makes a new line and the button sends.
                        if ev.key() == "Enter" && !ev.shift_key() && !ev.is_composing() && !util::is_touch() {
                            ev.prevent_default();
                            submit();
                        }
                    }
                ></textarea>
                <div class="composer-bar">
                    <input
                        node_ref=file_input
                        type="file"
                        multiple
                        hidden
                        accept="image/png,image/jpeg,image/gif,image/webp,application/pdf,text/*,.md,.csv,.json,.rs,.py,.js,.ts,.go,.java,.c,.cpp,.h,.toml,.yaml,.yml,.sh,.sql,.html,.css,.xml,.log"
                        on:change=move |_| {
                            if let Some(el) = file_input.get_untracked() {
                                ctx.add_files(files_from(el.files()));
                                el.set_value("");
                            }
                        }
                    />
                    <button
                        type="button"
                        class="icon-btn small"
                        title="Attach files (or paste / drop)"
                        aria-label="Attach files"
                        on:click=move |_| {
                            if let Some(el) = file_input.get_untracked() {
                                el.click();
                            }
                        }
                    >
                        <Icon svg=icons::CLIP size=16 />
                    </button>
                    <ToolsMenu />
                    <ModelPicker />
                    <Show
                        when=move || ctx.streaming.get()
                        fallback=move || {
                            view! {
                                <button class="send-btn" type="submit" aria-label="Send" disabled=move || !can_send()>
                                    <Icon svg=icons::SEND />
                                </button>
                            }
                        }
                    >
                        <button class="send-btn stop" type="button" aria-label="Stop" on:click=move |_| ctx.stop()>
                            <Icon svg=icons::STOP />
                        </button>
                    </Show>
                </div>
            </form>
            <p class="composer-hint muted">"Keys stay on your server · AI can make mistakes"</p>
        </div>
    }
}
