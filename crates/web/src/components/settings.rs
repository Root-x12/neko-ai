use leptos::{prelude::*, task::spawn_local};
use neko_shared::*;

use crate::{
    api,
    icons::{self, Icon},
    state::{Ctx, SettingsTab},
    theme::{Mode, Scheme, Theme, Variant, CYBER_VARIANTS, NEKO_VARIANTS},
    util,
};

#[component]
pub fn SettingsModal() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let tab = move || ctx.settings.get();
    let tab_btn = move |t: SettingsTab, label: &'static str, icon: &'static str| {
        view! {
            <button class="tab" class:active=move || tab() == Some(t) on:click=move |_| ctx.settings.set(Some(t))>
                <Icon svg=icon />
                <span>{label}</span>
            </button>
        }
    };
    view! {
        <Show when=move || tab().is_some()>
            <div class="modal-scrim" on:click=move |_| ctx.settings.set(None)></div>
            <div class="modal card" role="dialog" aria-modal="true" aria-label="Settings">
                <nav class="tabs">
                    {tab_btn(SettingsTab::Appearance, "Appearance", icons::PALETTE)}
                    {tab_btn(SettingsTab::Providers, "Providers", icons::KEY)}
                    {tab_btn(SettingsTab::Devices, "Devices", icons::DEVICE)}
                    {tab_btn(SettingsTab::Account, "Account", icons::USER)}
                </nav>
                <section class="modal-body">
                    <button class="icon-btn modal-close" aria-label="Close settings" on:click=move |_| ctx.settings.set(None)>
                        <Icon svg=icons::X />
                    </button>
                    {move || match tab() {
                        Some(SettingsTab::Appearance) => view! { <Appearance /> }.into_any(),
                        Some(SettingsTab::Providers) => view! { <Providers /> }.into_any(),
                        Some(SettingsTab::Devices) => view! { <Devices /> }.into_any(),
                        Some(SettingsTab::Account) => view! { <Account /> }.into_any(),
                        None => ().into_any(),
                    }}
                </section>
            </div>
        </Show>
    }
}

// ---------- appearance ----------

#[component]
fn Appearance() -> impl IntoView {
    let theme = expect_context::<Theme>();
    let mode_card = move |m: Mode, title: &'static str, desc: &'static str, icon: &'static str| {
        view! {
            <button class="mode-card" class:active=move || theme.mode.get() == m data-preview=if m == Mode::Neko { "neko" } else { "cyber" } on:click=move |_| theme.mode.set(m)>
                <Icon svg=icon size=22 />
                <strong>{title}</strong>
                <span class="muted small">{desc}</span>
            </button>
        }
    };
    let palette = move |list: &'static [Variant], sig: RwSignal<&'static str>, m: Mode| {
        list.iter()
            .map(|v| {
                let id = v.id;
                let [bg, surface, a1, a2] = v.swatch;
                view! {
                    <button
                        class="swatch"
                        class:active=move || sig.get() == id
                        title=v.blurb
                        on:click=move |_| {
                            sig.set(id);
                            theme.mode.set(m);
                        }
                    >
                        <span class="swatch-colors" style=format!("background:{bg}")>
                            <i style=format!("background:{surface}")></i>
                            <i style=format!("background:{a1}")></i>
                            <i style=format!("background:{a2}")></i>
                        </span>
                        <span>{v.name}</span>
                    </button>
                }
            })
            .collect_view()
    };
    let scheme_btn = move |s: Scheme, label: &'static str| {
        view! {
            <button class="seg" class:active=move || theme.scheme.get() == s on:click=move |_| theme.scheme.set(s)>{label}</button>
        }
    };
    view! {
        <h3>"Appearance"</h3>
        <p class="muted">"Two moods, each with its own palettes. The switch in the top bar flips between them."</p>
        <div class="mode-cards">
            {mode_card(Mode::Neko, "Neko", "Soft & cozy, for relaxing", icons::PAW)}
            {mode_card(Mode::Cyber, "Cyber", "Neon & sharp, for coding", icons::BOLT)}
        </div>
        <h4>"Neko palettes"</h4>
        <div class="swatches">{palette(NEKO_VARIANTS, theme.neko_variant, Mode::Neko)}</div>
        <h4>"Cyber palettes"</h4>
        <div class="swatches">{palette(CYBER_VARIANTS, theme.cyber_variant, Mode::Cyber)}</div>
        <h4>"Brightness"</h4>
        <div class="segmented">
            {scheme_btn(Scheme::Light, "Light")}
            {scheme_btn(Scheme::Dark, "Dark")}
            {scheme_btn(Scheme::System, "System")}
        </div>
        <h4>"Motion"</h4>
        <label class="check">
            <input type="checkbox" prop:checked=move || theme.motion.get() on:change=move |ev| theme.motion.set(event_target_checked(&ev)) />
            "Animated background and mascot"
        </label>
    }
}

// ---------- providers ----------

#[derive(Clone, Default)]
struct Form {
    editing: Option<i64>,
    name: String,
    kind: Option<ProviderKind>,
    base_url: String,
    api_key: String,
    remove_key: bool,
}

fn kind_label(k: ProviderKind) -> &'static str {
    match k {
        ProviderKind::OpenAiCompatible => "OpenAI-compatible",
        ProviderKind::Anthropic => "Anthropic",
        ProviderKind::Gemini => "Google Gemini",
    }
}

#[component]
fn Providers() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let form = RwSignal::new(None::<Form>);
    let busy = RwSignal::new(false);

    let apply_preset = move |idx: usize| {
        if let Some(p) = PROVIDER_PRESETS.get(idx) {
            form.update(|f| {
                if let Some(f) = f {
                    f.name = if p.base_url.is_empty() { String::new() } else { p.name.to_string() };
                    f.kind = Some(p.kind);
                    f.base_url = p.base_url.to_string();
                }
            });
        }
    };

    let save = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let Some(f) = form.get_untracked() else { return };
        let input = ProviderInput {
            name: f.name.clone(),
            kind: f.kind.unwrap_or(ProviderKind::OpenAiCompatible),
            base_url: f.base_url.clone(),
            api_key: if f.remove_key {
                Some(String::new())
            } else if f.api_key.trim().is_empty() {
                // On create, "no key" (local models); on edit, keep the stored key.
                f.editing.is_none().then(String::new)
            } else {
                Some(f.api_key.clone())
            },
        };
        busy.set(true);
        spawn_local(async move {
            let result = match f.editing {
                Some(id) => api::send::<_, Provider>("PUT", &format!("/providers/{id}"), &input).await,
                None => api::send::<_, Provider>("POST", "/providers", &input).await,
            };
            busy.set(false);
            match result {
                Ok(p) => {
                    let pid = p.id;
                    ctx.models.update(|m| {
                        m.remove(&pid);
                    });
                    form.set(None);
                    ctx.notify(format!("Saved {}", p.name), false);
                    ctx.load_providers();
                    if f.editing.is_none() || ctx.provider_id.get_untracked() == Some(pid) {
                        ctx.select_provider(pid);
                    }
                }
                Err(e) => ctx.error(e),
            }
        });
    };

    let delete = move |p: Provider| {
        if !util::confirm(&format!("Remove {}? Its API key is deleted from the server.", p.name)) {
            return;
        }
        spawn_local(async move {
            match api::call("DELETE", &format!("/providers/{}", p.id)).await {
                Ok(()) => {
                    if ctx.provider_id.get_untracked() == Some(p.id) {
                        ctx.provider_id.set(None);
                    }
                    ctx.load_providers();
                }
                Err(e) => ctx.error(e),
            }
        });
    };

    let field = move |label: &'static str,
                      get: fn(&Form) -> String,
                      set: fn(&mut Form, String),
                      kind: &'static str,
                      ph: &'static str| {
        view! {
            <label class="field">
                <span>{label}</span>
                <input
                    type=kind
                    placeholder=ph
                    spellcheck="false"
                    autocapitalize="none"
                    autocomplete="off"
                    prop:value=move || form.with(|f| f.as_ref().map(get).unwrap_or_default())
                    on:input=move |ev| {
                        let v = event_target_value(&ev);
                        form.update(|f| if let Some(f) = f { set(f, v) });
                    }
                />
            </label>
        }
    };

    view! {
        <h3>"AI providers"</h3>
        <p class="muted">
            "Bring a key from any provider. Keys are encrypted on your server and never sent back to the browser."
        </p>
        <div class="provider-list">
            <Show when=move || ctx.providers.with(|p| p.is_empty()) && form.with(|f| f.is_none())>
                <p class="muted">"No providers yet."</p>
            </Show>
            <For each=move || ctx.providers.get() key=|p| (p.id, p.name.clone(), p.base_url.clone(), p.key_hint.clone(), p.kind) let:p>
                {
                    let (pe, pd) = (p.clone(), p.clone());
                    let id = p.id;
                    view! {
                        <div class="provider-row">
                            <div class="provider-info">
                                <strong>{p.name.clone()}</strong>
                                <span class="muted small">{kind_label(p.kind)} " · " {p.base_url.clone()}</span>
                                <span class="muted small">
                                    {if p.has_key { format!("🔑 key {}", p.key_hint) } else { "no key".into() }}
                                </span>
                            </div>
                            <div class="row-actions">
                                <button class="btn small ghost" on:click=move |_| { ctx.select_provider(id); ctx.load_models(id, true); }>"Test"</button>
                                <button
                                    class="btn small ghost"
                                    on:click=move |_| form.set(Some(Form {
                                        editing: Some(pe.id),
                                        name: pe.name.clone(),
                                        kind: Some(pe.kind),
                                        base_url: pe.base_url.clone(),
                                        ..Default::default()
                                    }))
                                >
                                    "Edit"
                                </button>
                                <button class="btn small ghost danger" on:click=move |_| delete(pd.clone())>"Remove"</button>
                            </div>
                        </div>
                    }
                }
            </For>
        </div>
        <Show
            when=move || form.with(|f| f.is_some())
            fallback=move || view! {
                <button class="btn primary" on:click=move |_| { form.set(Some(Form::default())); apply_preset(0); }>
                    <Icon svg=icons::PLUS />
                    "Add provider"
                </button>
            }
        >
            <form class="provider-form card inset" on:submit=save>
                <h4>{move || if form.with(|f| f.as_ref().is_some_and(|f| f.editing.is_some())) { "Edit provider" } else { "Add provider" }}</h4>
                <Show when=move || form.with(|f| f.as_ref().is_some_and(|f| f.editing.is_none()))>
                    <label class="field">
                        <span>"Preset"</span>
                        <select on:change=move |ev| {
                            if let Ok(i) = event_target_value(&ev).parse::<usize>() {
                                apply_preset(i);
                            }
                        }>
                            {PROVIDER_PRESETS
                                .iter()
                                .enumerate()
                                .map(|(i, p)| view! { <option value=i.to_string()>{p.name}</option> })
                                .collect_view()}
                        </select>
                    </label>
                </Show>
                {field("Name", |f| f.name.clone(), |f, v| f.name = v, "text", "My provider")}
                <label class="field">
                    <span>"API format"</span>
                    <select
                        prop:value=move || form.with(|f| f.as_ref().and_then(|f| f.kind).unwrap_or(ProviderKind::OpenAiCompatible).as_str().to_string())
                        on:change=move |ev| {
                            let k = ProviderKind::parse(&event_target_value(&ev));
                            form.update(|f| if let Some(f) = f { f.kind = k });
                        }
                    >
                        <option value="open_ai_compatible">"OpenAI-compatible"</option>
                        <option value="anthropic">"Anthropic"</option>
                        <option value="gemini">"Google Gemini"</option>
                    </select>
                </label>
                {field("Base URL", |f| f.base_url.clone(), |f, v| f.base_url = v, "url", "https://api.example.com/v1")}
                {field("API key", |f| f.api_key.clone(), |f, v| f.api_key = v, "password", "sk-… (leave empty for local models)")}
                <Show when=move || form.with(|f| f.as_ref().is_some_and(|f| f.editing.is_some()))>
                    <p class="muted small">"Leave the key empty to keep the current one."</p>
                    <label class="check">
                        <input
                            type="checkbox"
                            prop:checked=move || form.with(|f| f.as_ref().is_some_and(|f| f.remove_key))
                            on:change=move |ev| {
                                let on = event_target_checked(&ev);
                                form.update(|f| if let Some(f) = f { f.remove_key = on });
                            }
                        />
                        "Remove the stored key"
                    </label>
                </Show>
                <div class="form-actions">
                    <button class="btn ghost" type="button" on:click=move |_| form.set(None)>"Cancel"</button>
                    <button class="btn primary" type="submit" disabled=move || busy.get()>"Save"</button>
                </div>
            </form>
        </Show>
    }
}

// ---------- devices ----------

#[component]
fn Devices() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let info = RwSignal::new(None::<DevicesInfo>);
    let reload = move || {
        spawn_local(async move {
            match api::get::<DevicesInfo>("/devices").await {
                Ok(i) => info.set(Some(i)),
                Err(e) => ctx.error(e),
            }
        })
    };
    reload();

    let revoke = move |id: i64| {
        spawn_local(async move {
            match api::call("DELETE", &format!("/devices/{id}")).await {
                Ok(()) => reload(),
                Err(e) => ctx.error(e),
            }
        })
    };
    let revoke_others = move |_| {
        if !util::confirm("Sign out all other devices?") {
            return;
        }
        spawn_local(async move {
            match api::call("POST", "/devices/revoke-others").await {
                Ok(()) => reload(),
                Err(e) => ctx.error(e),
            }
        })
    };

    view! {
        <h3>"Connect a device"</h3>
        <p class="muted">
            "Open Neko AI on your phone or another computer on the same Wi-Fi. Sign in with the same account and your chats follow you."
        </p>
        {move || info.get().map(|i| {
            let first = i.urls.first().cloned();
            view! {
                <div class="connect">
                    {i.qr_svg.clone().map(|svg| view! { <div class="qr" inner_html=svg></div> })}
                    <div class="connect-urls">
                        {i.urls.iter().map(|u| {
                            let u2 = u.clone();
                            view! {
                                <div class="url-row">
                                    <code>{u.clone()}</code>
                                    <button class="icon-btn small" aria-label="Copy address" on:click=move |_| { util::copy_text(&u2); ctx.notify("Address copied", false); }>
                                        <Icon svg=icons::COPY size=15 />
                                    </button>
                                </div>
                            }
                        }).collect_view()}
                        <p class="muted small">
                            {if first.as_deref().is_some_and(|u| u.contains("localhost")) {
                                "The server only listens on this computer. Start it with NEKO_BIND=0.0.0.0:8080 to allow other devices."
                            } else {
                                "Scan the QR code with your phone camera. Tip: on the phone, use “Add to Home Screen” to get an app icon."
                            }}
                        </p>
                    </div>
                </div>
                <div class="sessions-head">
                    <h4>"Signed-in devices"</h4>
                    <button class="btn small ghost" on:click=revoke_others disabled=i.sessions.len() < 2>"Sign out others"</button>
                </div>
                <div class="session-list">
                    {i.sessions.into_iter().map(|s| {
                        let id = s.id;
                        view! {
                            <div class="session-row">
                                <Icon svg=icons::DEVICE />
                                <div class="session-info">
                                    <strong>{util::device_label(&s.user_agent)} {s.current.then_some(" (this device)")}</strong>
                                    <span class="muted small">"Active " {util::ago(s.last_seen)} " · signed in " {util::ago(s.created_at)}</span>
                                </div>
                                {(!s.current).then(|| view! {
                                    <button class="btn small ghost danger" on:click=move |_| revoke(id)>"Sign out"</button>
                                })}
                            </div>
                        }
                    }).collect_view()}
                </div>
            }
        })}
    }
}

// ---------- account ----------

#[component]
fn Account() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let current = RwSignal::new(String::new());
    let new = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let change = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        busy.set(true);
        let body =
            ChangePasswordRequest { current_password: current.get_untracked(), new_password: new.get_untracked() };
        spawn_local(async move {
            let r = api::call_with("POST", "/auth/password", &body).await;
            busy.set(false);
            match r {
                Ok(()) => {
                    current.set(String::new());
                    new.set(String::new());
                    ctx.notify("Password changed. Other devices were signed out.", false);
                }
                Err(e) => ctx.error(e),
            }
        });
    };
    view! {
        <h3>"Account"</h3>
        <p>
            "Signed in as " <strong>{move || ctx.user.with(|u| u.username.clone())}</strong>
            " · " {move || ctx.user.with(|u| u.role.as_str())}
        </p>
        <form class="card inset" on:submit=change>
            <h4>"Change password"</h4>
            <label class="field">
                <span>"Current password"</span>
                <input type="password" autocomplete="current-password" required prop:value=move || current.get() on:input=move |ev| current.set(event_target_value(&ev)) />
            </label>
            <label class="field">
                <span>"New password"</span>
                <input type="password" autocomplete="new-password" minlength="8" required prop:value=move || new.get() on:input=move |ev| new.set(event_target_value(&ev)) />
            </label>
            <div class="form-actions">
                <button class="btn primary" type="submit" disabled=move || busy.get()>"Update password"</button>
            </div>
        </form>
        <h4>"Privacy"</h4>
        <p class="muted small">
            "Neko AI sends no telemetry. Your chats and keys live in the server's data folder; messages go only to the AI provider you pick."
        </p>
        <button class="btn danger-outline" on:click=move |_| ctx.sign_out()>"Sign out"</button>
    }
}
