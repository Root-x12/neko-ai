//! Theme state: Neko (relaxing) vs Cyber (coding), a palette for each, light/dark, motion.

use leptos::prelude::*;
use wasm_bindgen::{closure::Closure, JsCast};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Neko,
    Cyber,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    Light,
    Dark,
    System,
}

pub struct Variant {
    pub id: &'static str,
    pub name: &'static str,
    pub blurb: &'static str,
    /// Preview swatches: background, surface, accent, accent 2.
    pub swatch: [&'static str; 4],
}

pub const NEKO_VARIANTS: &[Variant] = &[
    Variant {
        id: "sakura",
        name: "Sakura",
        blurb: "cherry-blossom pink",
        swatch: ["#fff6f9", "#ffffff", "#f27aa5", "#a98bf5"],
    },
    Variant {
        id: "matcha",
        name: "Matcha",
        blurb: "calm green tea",
        swatch: ["#f5f8ef", "#ffffff", "#6aa84f", "#d9a45b"],
    },
    Variant {
        id: "mochi",
        name: "Mochi",
        blurb: "warm cream & peach",
        swatch: ["#fff8ef", "#ffffff", "#f29b5c", "#e7799a"],
    },
    Variant {
        id: "lavender",
        name: "Lavender",
        blurb: "sleepy purple dusk",
        swatch: ["#f7f4ff", "#ffffff", "#8b6cf0", "#5fb7d9"],
    },
];

pub const CYBER_VARIANTS: &[Variant] = &[
    Variant { id: "neon", name: "Neon", blurb: "cyan & magenta", swatch: ["#070b14", "#0d1424", "#22e4ff", "#ff3cac"] },
    Variant {
        id: "matrix",
        name: "Matrix",
        blurb: "terminal green",
        swatch: ["#030803", "#081208", "#39ff6a", "#b4ff39"],
    },
    Variant {
        id: "synthwave",
        name: "Synthwave",
        blurb: "sunset purple",
        swatch: ["#120a1f", "#1b1030", "#ff7edb", "#ffb347"],
    },
    Variant { id: "ice", name: "Ice", blurb: "cold steel blue", swatch: ["#060d14", "#0c1722", "#7cc4ff", "#c4a3ff"] },
];

#[derive(Clone, Copy)]
pub struct Theme {
    pub mode: RwSignal<Mode>,
    pub neko_variant: RwSignal<&'static str>,
    pub cyber_variant: RwSignal<&'static str>,
    pub scheme: RwSignal<Scheme>,
    pub motion: RwSignal<bool>,
    system_dark: RwSignal<bool>,
}

const STORAGE_KEY: &str = "neko.theme";

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

pub fn load_pref(key: &str) -> Option<String> {
    storage()?.get_item(key).ok().flatten()
}

pub fn save_pref(key: &str, value: &str) {
    if let Some(s) = storage() {
        let _ = s.set_item(key, value);
    }
}

fn find(list: &'static [Variant], id: &str) -> &'static str {
    list.iter().find(|v| v.id == id).unwrap_or(&list[0]).id
}

impl Theme {
    pub fn init() -> Self {
        let saved = load_pref(STORAGE_KEY).unwrap_or_default();
        let parts: Vec<&str> = saved.split('|').collect();
        let get = |i: usize| parts.get(i).copied().unwrap_or("");
        let window = web_sys::window().expect("window");
        let mq = window.match_media("(prefers-color-scheme: dark)").ok().flatten();
        let reduce_motion =
            window.match_media("(prefers-reduced-motion: reduce)").ok().flatten().is_some_and(|m| m.matches());

        let theme = Theme {
            mode: RwSignal::new(if get(0) == "cyber" { Mode::Cyber } else { Mode::Neko }),
            neko_variant: RwSignal::new(find(NEKO_VARIANTS, get(1))),
            cyber_variant: RwSignal::new(find(CYBER_VARIANTS, get(2))),
            scheme: RwSignal::new(match get(3) {
                "light" => Scheme::Light,
                "dark" => Scheme::Dark,
                _ => Scheme::System,
            }),
            motion: RwSignal::new(match get(4) {
                "0" => false,
                "1" => true,
                _ => !reduce_motion,
            }),
            system_dark: RwSignal::new(mq.as_ref().is_some_and(|m| m.matches())),
        };

        if let Some(mq) = mq {
            let sd = theme.system_dark;
            let cb =
                Closure::<dyn Fn(web_sys::MediaQueryList)>::new(move |m: web_sys::MediaQueryList| sd.set(m.matches()));
            mq.set_onchange(Some(cb.as_ref().unchecked_ref()));
            cb.forget(); // lives for the whole app
        }

        Effect::new(move |_| theme.apply());
        theme
    }

    pub fn is_dark(&self) -> bool {
        match self.scheme.get() {
            Scheme::Light => false,
            Scheme::Dark => true,
            Scheme::System => self.system_dark.get(),
        }
    }

    pub fn variant(&self) -> &'static str {
        match self.mode.get() {
            Mode::Neko => self.neko_variant.get(),
            Mode::Cyber => self.cyber_variant.get(),
        }
    }

    pub fn toggle_mode(&self) {
        self.mode.update(|m| *m = if *m == Mode::Neko { Mode::Cyber } else { Mode::Neko });
    }

    pub fn toggle_dark(&self) {
        let dark = self.is_dark();
        self.scheme.set(if dark { Scheme::Light } else { Scheme::Dark });
    }

    fn apply(&self) {
        let mode = if self.mode.get() == Mode::Cyber { "cyber" } else { "neko" };
        let variant = self.variant();
        let dark = self.is_dark();
        let motion = self.motion.get();
        if let Some(root) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
            let _ = root.set_attribute("data-mode", mode);
            let _ = root.set_attribute("data-variant", variant);
            let _ = root.set_attribute("data-scheme", if dark { "dark" } else { "light" });
            let _ = root.set_attribute("data-motion", if motion { "on" } else { "off" });
        }
        let scheme = match self.scheme.get() {
            Scheme::Light => "light",
            Scheme::Dark => "dark",
            Scheme::System => "system",
        };
        save_pref(
            STORAGE_KEY,
            &format!(
                "{mode}|{}|{}|{scheme}|{}",
                self.neko_variant.get(),
                self.cyber_variant.get(),
                if motion { "1" } else { "0" }
            ),
        );
    }
}
