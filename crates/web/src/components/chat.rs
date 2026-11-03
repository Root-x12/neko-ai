use leptos::{html, prelude::*};
use neko_shared::MsgRole;
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

    view! {
        <article
            class="msg"
            class:user=is_user
            class:assistant=!is_user
            class:streaming=move || m.streaming.get()
            class:editing=move || ctx.editing.get() == Some(m.id)
        >
            <Show when=move || !is_user>
                <div class="msg-meta">
                    <span class="msg-avatar"><Icon svg=icons::PAW size=14 /></span>
                    <span class="msg-model">{move || m.model.get().unwrap_or_else(|| "assistant".into())}</span>
                </div>
            </Show>
            <div class="msg-body">
                <Show when=move || m.streaming.get() && m.content.with(|c| c.is_empty())>
                    <div class="typing" aria-label="Thinking"><span></span><span></span><span></span></div>
                </Show>
                <div class="md" inner_html=move || html.get()></div>
                {move || m.error.get().map(|e| {
                    let stopped = e == "Stopped";
                    view! { <div class="msg-error" class:soft=stopped>{if stopped { "⏹ Stopped".to_string() } else { format!("⚠ {e}") }}</div> }
                })}
            </div>
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
                            on:click=move |_| ctx.start_edit(m.id, m.content.get_untracked())
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
fn Composer() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let input = NodeRef::<html::Textarea>::new();

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
    // Focus the box when editing starts or a suggestion is picked.
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

    view! {
        <div class="composer-wrap">
            <Show when=move || ctx.editing.get().is_some()>
                <div class="edit-banner">
                    <Icon svg=icons::EDIT size=14 />
                    "Editing — sending replaces this message and everything after it"
                    <button
                        class="icon-btn small"
                        aria-label="Cancel edit"
                        on:click=move |_| {
                            ctx.editing.set(None);
                            ctx.draft.set(String::new());
                        }
                    >
                        <Icon svg=icons::X size=14 />
                    </button>
                </div>
            </Show>
            <form
                class="composer"
                on:submit=move |ev| {
                    ev.prevent_default();
                    submit();
                }
            >
                <textarea
                    node_ref=input
                    rows="1"
                    placeholder=move || if ctx.theme.mode.get() == Mode::Cyber { "$ ask anything…" } else { "Message Neko…" }
                    aria-label="Message"
                    prop:value=move || ctx.draft.get()
                    on:input=move |ev| ctx.draft.set(event_target_value(&ev))
                    on:keydown=move |ev| {
                        // Enter sends on desktop; on phones Enter makes a new line and the button sends.
                        if ev.key() == "Enter" && !ev.shift_key() && !ev.is_composing() && !util::is_touch() {
                            ev.prevent_default();
                            submit();
                        }
                    }
                ></textarea>
                <div class="composer-bar">
                    <ModelPicker />
                    <Show
                        when=move || ctx.streaming.get()
                        fallback=move || view! {
                            <button
                                class="send-btn"
                                type="submit"
                                aria-label="Send"
                                disabled=move || ctx.draft.with(|d| d.trim().is_empty())
                            >
                                <Icon svg=icons::SEND />
                            </button>
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
