use leptos::prelude::*;

/// The cat. One SVG for both themes: CSS turns it soft and blushing (Neko) or neon with a
/// scanning visor (Cyber). `state` is "idle", "thinking" or "sleep".
#[component]
pub fn Mascot(#[prop(into)] state: Signal<&'static str>, #[prop(optional)] big: bool) -> impl IntoView {
    view! {
        <div class="mascot" class:big=big data-state=move || state.get() aria-hidden="true">
            <svg viewBox="0 0 120 104">
                <g class="tail">
                    <path d="M98 88 C 116 84, 118 64, 106 58" />
                </g>
                <g class="ear ear-l">
                    <path class="ear-outer" d="M22 48 L 26 8 L 52 30 Z" />
                    <path class="ear-inner" d="M28 38 L 30 18 L 44 31 Z" />
                </g>
                <g class="ear ear-r">
                    <path class="ear-outer" d="M98 48 L 94 8 L 68 30 Z" />
                    <path class="ear-inner" d="M92 38 L 90 18 L 76 31 Z" />
                </g>
                <ellipse class="head" cx="60" cy="60" rx="44" ry="36" />
                <ellipse class="blush" cx="32" cy="70" rx="7" ry="4" />
                <ellipse class="blush" cx="88" cy="70" rx="7" ry="4" />
                <g class="eyes">
                    <g class="eye-wrap">
                        <ellipse class="eye" cx="42" cy="58" rx="6" ry="8" />
                        <circle class="shine" cx="44.5" cy="54.5" r="2.2" />
                    </g>
                    <g class="eye-wrap">
                        <ellipse class="eye" cx="78" cy="58" rx="6" ry="8" />
                        <circle class="shine" cx="80.5" cy="54.5" r="2.2" />
                    </g>
                    <path class="sleep-eye" d="M35 59 Q 42 64 49 59" />
                    <path class="sleep-eye" d="M71 59 Q 78 64 85 59" />
                </g>
                <rect class="visor" x="26" y="49" width="68" height="18" rx="9" />
                <rect class="visor-scan" x="28" y="51" width="10" height="14" rx="5" />
                <path class="nose" d="M56.5 70 h7 l-3.5 4 z" />
                <path class="mouth" d="M60 74 q -4 5 -9 2 M60 74 q 4 5 9 2" />
                <g class="whiskers">
                    <path d="M14 66 L 32 69 M13 74 L 32 74 M86 69 L 104 66 M86 74 L 105 74" />
                </g>
                <g class="zzz">
                    <text x="96" y="22">"z"</text>
                    <text x="104" y="12">"z"</text>
                </g>
            </svg>
        </div>
    }
}

/// Decorative animated background: drifting petals/paws (Neko) or a neon grid (Cyber).
#[component]
pub fn Backdrop() -> impl IntoView {
    // Deterministic pseudo-random layout so it looks organic without a RNG.
    let petals = (0..14)
        .map(|i| {
            let left = (i * 37 + 11) % 100;
            let delay = (i * 7) % 19;
            let dur = 16 + (i * 5) % 14;
            let size = 10 + (i * 3) % 12;
            let style = format!("left:{left}%;animation-delay:-{delay}s;animation-duration:{dur}s;--s:{size}px");
            let class = if i % 3 == 0 { "petal paw" } else { "petal" };
            view! { <span class=class style=style></span> }
        })
        .collect_view();
    view! {
        <div class="backdrop" aria-hidden="true">
            <div class="neko-bg">{petals}</div>
            <div class="cyber-bg">
                <div class="grid"></div>
                <div class="glow"></div>
                <div class="scanlines"></div>
            </div>
        </div>
    }
}
