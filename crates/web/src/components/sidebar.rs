use std::time::Duration;

use leptos::{prelude::*, task::spawn_local};
use neko_shared::SearchHit;

use crate::{
    api,
    components::mascot::Mascot,
    icons::{self, Icon},
    markdown,
    state::{Ctx, SettingsTab},
};

#[component]
pub fn Sidebar() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let query = RwSignal::new(String::new());
    let hits = RwSignal::new(None::<Vec<SearchHit>>);
    let search_seq = StoredValue::new(0u32);

    // Debounced server-side search over titles and message text.
    let on_search = move |ev| {
        let q = event_target_value(&ev);
        query.set(q.clone());
        let seq = search_seq.get_value() + 1;
        search_seq.set_value(seq);
        if q.trim().is_empty() {
            hits.set(None);
            return;
        }
        set_timeout(
            move || {
                if search_seq.get_value() != seq {
                    return;
                }
                spawn_local(async move {
                    let path = format!("/search?q={}", String::from(js_sys::encode_uri_component(&q)));
                    match api::get::<Vec<SearchHit>>(&path).await {
                        Ok(list) if search_seq.get_value() == seq => hits.set(Some(list)),
                        Ok(_) => {}
                        Err(e) => ctx.error(e),
                    }
                });
            },
            Duration::from_millis(220),
        );
    };

    let clear_search = move || {
        query.set(String::new());
        hits.set(None);
        search_seq.update_value(|s| *s += 1);
    };

    view! {
        <aside class="sidebar" class:open=move || ctx.sidebar_open.get()>
            <div class="sidebar-head">
                <div class="brand">
                    <Mascot state=Signal::derive(move || {
                        if ctx.streaming.get() { "thinking" } else if ctx.idle.get() { "sleep" } else { "idle" }
                    }) />
                    <span class="brand-name">"Neko AI"</span>
                </div>
                <button class="icon-btn only-mobile" aria-label="Close menu" on:click=move |_| ctx.sidebar_open.set(false)>
                    <Icon svg=icons::X />
                </button>
            </div>
            <button class="btn primary new-chat" on:click=move |_| { clear_search(); ctx.new_chat(); }>
                <Icon svg=icons::PLUS />
                "New chat"
            </button>
            <label class="search">
                <Icon svg=icons::SEARCH size=16 />
                <input
                    type="search"
                    placeholder="Search chats"
                    aria-label="Search chats"
                    prop:value=move || query.get()
                    on:input=on_search
                />
            </label>
            <nav class="conv-list" aria-label="Chats">
                {move || match hits.get() {
                    Some(list) if list.is_empty() => view! { <p class="muted empty-list">"No matches"</p> }.into_any(),
                    Some(list) => list
                        .into_iter()
                        .map(|h| {
                            let id = h.conversation_id;
                            view! {
                                <button
                                    class="conv-item hit"
                                    class:active=move || ctx.current_id.get() == Some(id)
                                    on:click=move |_| ctx.open_conversation(id)
                                >
                                    <span class="conv-title">{h.title}</span>
                                    <span class="conv-snippet" inner_html=markdown::snippet_html(&h.snippet)></span>
                                </button>
                            }
                        })
                        .collect_view()
                        .into_any(),
                    None => view! {
                        <Show when=move || ctx.conversations.with(|l| l.is_empty())>
                            <p class="muted empty-list">"No chats yet. Say hi!"</p>
                        </Show>
                        <For
                            each=move || ctx.conversations.get()
                            key=|c| (c.id, c.title.clone())
                            let:c
                        >
                            {
                                let id = c.id;
                                let title = c.title.clone();
                                let tooltip = c.title.clone();
                                view! {
                                    <div class="conv-row" class:active=move || ctx.current_id.get() == Some(id)>
                                        <button class="conv-item" on:click=move |_| ctx.open_conversation(id) title=tooltip>
                                            <span class="conv-title">{c.title}</span>
                                        </button>
                                        <div class="conv-actions">
                                            <button
                                                class="icon-btn small"
                                                aria-label="Rename chat"
                                                on:click=move |_| ctx.rename_conversation(id, title.clone())
                                            >
                                                <Icon svg=icons::EDIT size=15 />
                                            </button>
                                            <button
                                                class="icon-btn small danger"
                                                aria-label="Delete chat"
                                                on:click=move |_| ctx.delete_conversation(id)
                                            >
                                                <Icon svg=icons::TRASH size=15 />
                                            </button>
                                        </div>
                                    </div>
                                }
                            }
                        </For>
                    }
                    .into_any(),
                }}
            </nav>
            <div class="sidebar-foot">
                <button class="user-chip" on:click=move |_| ctx.settings.set(Some(SettingsTab::Account))>
                    <span class="avatar">{move || ctx.user.with(|u| u.username.chars().next().unwrap_or('?').to_uppercase().to_string())}</span>
                    <span class="user-name">{move || ctx.user.with(|u| u.username.clone())}</span>
                </button>
                <Show when=move || ctx.info.with(|i| i.as_ref().is_some_and(|i| i.local_only))>
                    <span class="local-badge" title="Local-only mode: nothing leaves your network">"LOCAL"</span>
                </Show>
                <button class="icon-btn" aria-label="Devices" title="Connect a device" on:click=move |_| ctx.settings.set(Some(SettingsTab::Devices))>
                    <Icon svg=icons::DEVICE />
                </button>
                <button class="icon-btn" aria-label="Settings" title="Settings" on:click=move |_| ctx.settings.set(Some(SettingsTab::Appearance))>
                    <Icon svg=icons::GEAR />
                </button>
            </div>
        </aside>
        <div class="scrim" class:show=move || ctx.sidebar_open.get() on:click=move |_| ctx.sidebar_open.set(false)></div>
    }
}
