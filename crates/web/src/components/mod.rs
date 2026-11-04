pub mod admin;
pub mod canvas;
pub mod chat;
pub mod guard;
pub mod library;
pub mod login;
pub mod mascot;
pub mod params;
pub mod scene;
pub mod settings;
pub mod sidebar;

use leptos::prelude::*;

use crate::{
    icons::{self, Icon},
    theme::{Mode, Theme},
};

/// The two always-visible switches: Neko ⇄ Cyber, and light ⇄ dark.
#[component]
pub fn ThemeToggles() -> impl IntoView {
    let theme = expect_context::<Theme>();
    let cyber = move || theme.mode.get() == Mode::Cyber;
    view! {
        <div class="theme-toggles">
            <button
                type="button"
                class="mode-switch"
                class:on=cyber
                role="switch"
                aria-checked=move || cyber().to_string()
                title=move || if cyber() { "Cyber mode (coding) — switch to Neko" } else { "Neko mode (relax) — switch to Cyber" }
                on:click=move |_| theme.toggle_mode()
            >
                <span class="mode-switch-label neko-label"><Icon svg=icons::PAW size=14 /></span>
                <span class="mode-switch-label cyber-label"><Icon svg=icons::BOLT size=14 /></span>
                <span class="mode-switch-knob"></span>
            </button>
            <button
                type="button"
                class="icon-btn"
                title=move || if theme.is_dark() { "Switch to light" } else { "Switch to dark" }
                aria-label="Toggle light or dark"
                on:click=move |_| theme.toggle_dark()
            >
                {move || if theme.is_dark() { view! { <Icon svg=icons::SUN /> } } else { view! { <Icon svg=icons::MOON /> } }}
            </button>
        </div>
    }
}
