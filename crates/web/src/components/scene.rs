//! The animated scenery behind the app. Every palette has its own world (`worlds.rs`); the two
//! default palettes share the blossom valley (`valley.rs`). SVG + CSS animation only, with
//! colours from CSS variables so each world has a day and a night look.

use leptos::prelude::*;

use crate::{
    components::{
        valley,
        worlds::{self, rnd},
    },
    theme::{Mode, Theme},
};

/// Leaves, petals, sparks or snow carried by the wind.
fn wind_items(class: &'static str, count: u32, seed: u32) -> impl IntoView {
    (0..count)
        .map(|i| {
            let top = 4.0 + rnd(seed + i) * 70.0;
            let left = rnd(seed + i + 250) * 100.0;
            let delay = rnd(seed + i + 50) * 30.0;
            let dur = 18.0 + rnd(seed + i + 100) * 16.0;
            let size = 9.0 + rnd(seed + i + 150) * 10.0;
            let flutter = 1.2 + rnd(seed + i + 200) * 1.4;
            let alt = if i % 3 == 0 { " alt" } else { "" };
            let style = format!(
                "top:{top:.1}%;--x:{left:.1}%;animation-delay:-{delay:.1}s;animation-duration:{dur:.1}s;--s:{size:.1}px;--f:{flutter:.2}s"
            );
            view! {
                <span class="wind" style=style>
                    <i class=format!("{class}{alt}")></i>
                </span>
            }
        })
        .collect_view()
}

fn neko_world(variant: &str) -> String {
    match variant {
        "sakura" => worlds::sakura(),
        "matcha" => worlds::matcha(),
        "mochi" => worlds::mochi(),
        "lavender" => worlds::lavender(),
        _ => valley::valley(false),
    }
}

fn cyber_world(variant: &str) -> String {
    match variant {
        "matrix" => worlds::matrix(),
        "synthwave" => worlds::synthwave(),
        "ice" => worlds::ice(),
        _ => valley::valley(true),
    }
}

fn wind_kind(mode: Mode, variant: &str) -> Option<&'static str> {
    match (mode, variant) {
        (Mode::Neko, "sakura" | "lavender") => Some("petal"),
        (Mode::Neko, "matcha") => Some("bamboo-leaf"),
        (Mode::Neko, "mochi") => Some("leaf"),
        (Mode::Cyber, "matrix") => None,
        (Mode::Cyber, "ice") => Some("snow"),
        (Mode::Cyber, "synthwave") => Some("spark"),
        // The blossom valley brings its own petals.
        _ => None,
    }
}

/// The two default palettes (Tabby and Neon) show the blossom valley.
fn in_valley(variant: &str) -> bool {
    matches!(variant, "tabby" | "neon")
}

/// Sky life drawn over a user's own picture: drifting cloud wisps and a passing flock.
fn wallpaper_fx() -> String {
    format!(
        r#"<svg class="scene wall-fx" viewBox="0 0 1600 900" preserveAspectRatio="xMidYMid slice"><g class="w-clouds drift-a fx-clouds">{}</g><g class="w-flock-path">{}</g></svg>"#,
        worlds::cloud_row(4242, 170.0, 5, 240.0),
        worlds::birds(4343, 8, 220.0)
    )
}

#[component]
pub fn Backdrop() -> impl IntoView {
    let theme = expect_context::<Theme>();
    // Only the active world is in the page, so switching palettes swaps the whole scene.
    let wallpaper = move || {
        let slot = theme.variant();
        theme
            .wallpapers
            .with(|w| w.iter().find(|x| x.slot == slot).map(|x| format!("/api/wallpapers/{slot}?v={}", x.updated_at)))
    };
    let scene = move || {
        if wallpaper().is_some() {
            return String::new();
        }
        match theme.mode.get() {
            Mode::Neko => neko_world(theme.neko_variant.get()),
            Mode::Cyber => cyber_world(theme.cyber_variant.get()),
        }
    };
    view! {
        <div class="backdrop" aria-hidden="true">
            <div class="scene-host" inner_html=scene></div>
            {move || {
                wallpaper()
                    .map(|url| {
                        view! {
                            <div class="wallpaper" style=format!("background-image:url('{url}')")></div>
                            <div class="scene-host" inner_html=wallpaper_fx()></div>
                        }
                    })
            }}
            {move || {
                let mode = theme.mode.get();
                wind_kind(mode, theme.variant())
                    .map(|k| view! { <div class=format!("winds wind-{k}")>{wind_items(k, if k == "snow" { 40 } else { 16 }, 10)}</div> })
            }}
            {move || {
                let cyber = theme.mode.get() == Mode::Cyber;
                (wallpaper().is_none() && in_valley(theme.variant()))
                    .then(|| {
                        let tone = if cyber { "va-neon" } else { "va-day" };
                        view! {
                            <div class=format!("winds wind-fall {tone}")>{wind_items("petal", 16, 700)}</div>
                            <div class=format!("winds wind-gust {tone}")>{wind_items("petal", 8, 800)}</div>
                            <div class=format!("winds wind-fall wind-bokeh {tone}")>{wind_items("petal", 5, 900)}</div>
                        }
                    })
            }}
            {move || (theme.mode.get() == Mode::Cyber).then(|| view! { <div class="scanlines"></div> })}
            <div class="veil"></div>
        </div>
    }
}
