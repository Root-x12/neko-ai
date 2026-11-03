use leptos::prelude::*;
use neko_shared::ChatParams;

use crate::{
    icons::{self, Icon},
    state::Ctx,
};

/// One optional numeric parameter: a checkbox to override the provider default, plus a slider.
#[component]
fn ParamSlider(
    label: &'static str,
    hint: &'static str,
    min: f32,
    max: f32,
    step: f32,
    default: f32,
    get: fn(&ChatParams) -> Option<f32>,
    set: fn(&mut ChatParams, Option<f32>),
) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let value = move || ctx.params.with(get);
    view! {
        <div class="param">
            <label class="param-head">
                <input
                    type="checkbox"
                    prop:checked=move || value().is_some()
                    on:change=move |ev| {
                        let on = event_target_checked(&ev);
                        ctx.params.update(|p| set(p, on.then_some(default)));
                    }
                />
                <span>{label}</span>
                <span class="param-value">{move || value().map(|v| format!("{v:.2}")).unwrap_or_else(|| "default".into())}</span>
            </label>
            <input
                type="range"
                min=min
                max=max
                step=step
                disabled=move || value().is_none()
                prop:value=move || value().unwrap_or(default).to_string()
                on:input=move |ev| {
                    if let Ok(v) = event_target_value(&ev).parse::<f32>() {
                        ctx.params.update(|p| set(p, Some(v)));
                    }
                }
            />
            <p class="muted small">{hint}</p>
        </div>
    }
}

#[component]
pub fn ParamsPanel() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    view! {
        <aside class="drawer" class:open=move || ctx.params_open.get() aria-label="Chat settings">
            <div class="drawer-head">
                <h3>"Chat settings"</h3>
                <button class="icon-btn" aria-label="Close" on:click=move |_| ctx.params_open.set(false)>
                    <Icon svg=icons::X />
                </button>
            </div>
            <div class="drawer-body">
                <label class="field">
                    <span>"System prompt"</span>
                    <textarea
                        rows="6"
                        placeholder="e.g. You are a friendly cat who explains things simply."
                        prop:value=move || ctx.params.with(|p| p.system_prompt.clone())
                        on:input=move |ev| {
                            let v = event_target_value(&ev);
                            ctx.params.update(|p| p.system_prompt = v);
                        }
                    ></textarea>
                </label>
                <ParamSlider
                    label="Temperature"
                    hint="Higher = more creative, lower = more focused."
                    min=0.0
                    max=2.0
                    step=0.05
                    default=0.7
                    get=|p| p.temperature
                    set=|p, v| p.temperature = v
                />
                <ParamSlider
                    label="Top-p"
                    hint="Nucleus sampling. Usually leave one of temperature/top-p at default."
                    min=0.0
                    max=1.0
                    step=0.01
                    default=0.9
                    get=|p| p.top_p
                    set=|p, v| p.top_p = v
                />
                <div class="param">
                    <label class="param-head">
                        <input
                            type="checkbox"
                            prop:checked=move || ctx.params.with(|p| p.max_tokens.is_some())
                            on:change=move |ev| {
                                let on = event_target_checked(&ev);
                                ctx.params.update(|p| p.max_tokens = on.then_some(4096));
                            }
                        />
                        <span>"Max output tokens"</span>
                    </label>
                    <input
                        type="number"
                        min="1"
                        max="1000000"
                        disabled=move || ctx.params.with(|p| p.max_tokens.is_none())
                        prop:value=move || ctx.params.with(|p| p.max_tokens.unwrap_or(4096).to_string())
                        on:change=move |ev| {
                            if let Ok(v) = event_target_value(&ev).parse::<u32>() {
                                ctx.params.update(|p| p.max_tokens = Some(v.clamp(1, 1_000_000)));
                            }
                        }
                    />
                </div>
                <p class="muted small">"Settings are saved with the chat when you send a message, and apply from the next reply on."</p>
                <button class="btn ghost" on:click=move |_| ctx.params.set(ChatParams::default())>"Reset to defaults"</button>
            </div>
        </aside>
    }
}
