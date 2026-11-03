//! Inline SVG icons (stroke = currentColor), so the app needs no icon font or network fetch.

use leptos::prelude::*;

const fn svg(body: &'static str) -> &'static str {
    body
}

pub const PLUS: &str = svg(r#"<path d="M12 5v14M5 12h14"/>"#);
pub const MENU: &str = svg(r#"<path d="M4 7h16M4 12h16M4 17h16"/>"#);
pub const SEARCH: &str = svg(r#"<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>"#);
pub const GEAR: &str = svg(
    r#"<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/>"#,
);
pub const SLIDERS: &str = svg(
    r#"<path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0"/><circle cx="16" cy="6" r="2"/><circle cx="10" cy="12" r="2"/><circle cx="18" cy="18" r="2"/>"#,
);
pub const SEND: &str = svg(r#"<path d="M5 12h14M13 6l6 6-6 6"/>"#);
pub const STOP: &str = svg(r#"<rect x="7" y="7" width="10" height="10" rx="2"/>"#);
pub const COPY: &str = svg(r#"<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h8"/>"#);
pub const REFRESH: &str = svg(r#"<path d="M20 11a8 8 0 0 0-14.9-4M4 4v4h4M4 13a8 8 0 0 0 14.9 4M20 20v-4h-4"/>"#);
pub const EDIT: &str = svg(r#"<path d="M12 20h9M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z"/>"#);
pub const TRASH: &str = svg(r#"<path d="M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14"/>"#);
pub const X: &str = svg(r#"<path d="M6 6l12 12M18 6 6 18"/>"#);
pub const SUN: &str = svg(
    r#"<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>"#,
);
pub const MOON: &str = svg(r#"<path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/>"#);
pub const PAW: &str = svg(
    r#"<circle cx="7" cy="9" r="2"/><circle cx="12" cy="6.5" r="2"/><circle cx="17" cy="9" r="2"/><path d="M8 17c0-2.5 2-5 4-5s4 2.5 4 5c0 1.7-1.8 2.3-4 1.6-2.2.7-4 .1-4-1.6z"/>"#,
);
pub const BOLT: &str = svg(r#"<path d="M13 2 4 14h7l-1 8 9-12h-7z"/>"#);
pub const DEVICE: &str = svg(
    r#"<rect x="2" y="4" width="14" height="10" rx="1.5"/><path d="M6 18h6"/><rect x="17" y="8" width="5" height="12" rx="1"/>"#,
);
pub const KEY: &str = svg(r#"<circle cx="8" cy="15" r="4"/><path d="m11 12 9-9M17 6l3 3M15 8l2 2"/>"#);
pub const PALETTE: &str = svg(
    r#"<path d="M12 3a9 9 0 1 0 0 18c1.1 0 1.6-.8 1.6-1.6 0-.5-.2-.8-.5-1.2-.3-.3-.4-.7-.4-1.1 0-.9.7-1.6 1.6-1.6H16a5 5 0 0 0 5-5c0-4.1-4-7.5-9-7.5z"/><circle cx="7.5" cy="11" r="1"/><circle cx="10.5" cy="7.5" r="1"/><circle cx="15" cy="8" r="1"/>"#,
);
pub const USER: &str = svg(r#"<circle cx="12" cy="8" r="4"/><path d="M4 21c1.5-4 4.5-6 8-6s6.5 2 8 6"/>"#);
pub const CLIP: &str = svg(
    r#"<path d="m21 11-8.6 8.6a5 5 0 0 1-7-7l8.5-8.6a3.3 3.3 0 0 1 4.7 4.7L10 17.3a1.7 1.7 0 0 1-2.4-2.4l7.9-7.9"/>"#,
);
pub const WRENCH: &str = svg(
    r#"<path d="M14.7 6.3a4 4 0 0 0 5 5L21 13l-8 8-3-3 1.7-1.7a4 4 0 0 0-5-5L3 7.6 7.6 3l3.7 3.7a4 4 0 0 0 3.4-.4z"/>"#,
);
pub const GLOBE: &str =
    svg(r#"<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/>"#);
pub const FILE: &str =
    svg(r#"<path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><path d="M14 3v6h6"/>"#);
pub const PDF: &str =
    svg(r#"<path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><path d="M14 3v6h6M8 15h8M8 18h5"/>"#);
pub const PLAY: &str = svg(r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="m10 9 5 3-5 3z"/>"#);
pub const DOWNLOAD: &str = svg(r#"<path d="M12 4v11M7 10l5 5 5-5M5 20h14"/>"#);
pub const EXPAND: &str = svg(r#"<path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/>"#);
pub const SHIELD: &str =
    svg(r#"<path d="M12 3 4 6v6c0 4.5 3.4 8.3 8 9 4.6-.7 8-4.5 8-9V6z"/><path d="m9 12 2 2 4-4"/>"#);

#[component]
pub fn Icon(#[prop(into)] svg: &'static str, #[prop(optional)] size: Option<u32>) -> impl IntoView {
    let size = size.unwrap_or(18);
    view! {
        <svg
            class="icon"
            width=size
            height=size
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
            inner_html=svg
        ></svg>
    }
}
