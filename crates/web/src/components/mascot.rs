use leptos::prelude::*;

/// Gradients shared by every cat on the page. Rendered once (not `display: none`, which would
/// stop some browsers from painting the gradients). Colours come from CSS variables, so every
/// palette shades the cat its own way.
#[component]
pub fn MascotDefs() -> impl IntoView {
    view! {
        <svg class="mascot-defs" width="0" height="0" aria-hidden="true" focusable="false">
            <defs>
                <radialGradient id="m-fur" cx="38%" cy="30%" r="75%">
                    <stop offset="0%" style="stop-color: var(--fur-hi)" />
                    <stop offset="55%" style="stop-color: var(--fur)" />
                    <stop offset="100%" style="stop-color: var(--fur-lo)" />
                </radialGradient>
                <radialGradient id="m-body" cx="40%" cy="20%" r="85%">
                    <stop offset="0%" style="stop-color: var(--fur)" />
                    <stop offset="100%" style="stop-color: var(--fur-lo)" />
                </radialGradient>
                <linearGradient id="m-ear" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" style="stop-color: var(--fur-hi)" />
                    <stop offset="100%" style="stop-color: var(--fur-lo)" />
                </linearGradient>
                <linearGradient id="m-ear-in" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" style="stop-color: var(--ear-in-hi)" />
                    <stop offset="100%" style="stop-color: var(--ear-in)" />
                </linearGradient>
                <radialGradient id="m-muzzle" cx="45%" cy="30%" r="80%">
                    <stop offset="0%" style="stop-color: var(--muzzle-hi)" />
                    <stop offset="100%" style="stop-color: var(--muzzle)" />
                </radialGradient>
                <radialGradient id="m-eye" cx="45%" cy="40%" r="65%">
                    <stop offset="0%" style="stop-color: var(--iris-hi)" />
                    <stop offset="45%" style="stop-color: var(--iris)" />
                    <stop offset="100%" style="stop-color: #120d10" />
                </radialGradient>
                <radialGradient id="m-nose" cx="40%" cy="30%" r="80%">
                    <stop offset="0%" style="stop-color: var(--nose-hi)" />
                    <stop offset="100%" style="stop-color: var(--nose)" />
                </radialGradient>
                <radialGradient id="m-gloss" cx="50%" cy="50%" r="50%">
                    <stop offset="0%" stop-color="#fff" stop-opacity=".7" />
                    <stop offset="100%" stop-color="#fff" stop-opacity="0" />
                </radialGradient>
                <radialGradient id="m-shadow" cx="50%" cy="50%" r="50%">
                    <stop offset="0%" stop-color="#000" stop-opacity=".35" />
                    <stop offset="100%" stop-color="#000" stop-opacity="0" />
                </radialGradient>
                <linearGradient id="m-metal" x1="0" y1="0" x2="1" y2="1">
                    <stop offset="0%" style="stop-color: var(--metal-hi)" />
                    <stop offset="60%" style="stop-color: var(--metal)" />
                    <stop offset="100%" style="stop-color: var(--metal-lo)" />
                </linearGradient>
            </defs>
        </svg>
    }
}

/// The cat. A shaded, softly 3D figure whose head turns and eyes follow the pointer (see
/// `--look-x/y`). CSS restyles the same shapes into a chrome robot cat with a visor in Cyber mode.
/// `state` is "idle", "thinking" or "sleep".
#[component]
pub fn Mascot(#[prop(into)] state: Signal<&'static str>, #[prop(optional)] big: bool) -> impl IntoView {
    view! {
        <div class="mascot" class:big=big data-state=move || state.get() aria-hidden="true">
            <svg viewBox="0 0 120 112">
                <ellipse class="shadow" cx="60" cy="104" rx="38" ry="6" />
                <g class="tail">
                    <path d="M88 92 C 112 90, 116 66, 102 58" />
                </g>
                <ellipse class="body" cx="60" cy="88" rx="31" ry="17" />
                <ellipse class="paw" cx="46" cy="100" rx="9" ry="5.5" />
                <ellipse class="paw" cx="74" cy="100" rx="9" ry="5.5" />
                <g class="head-group">
                    <g class="ear ear-l">
                        <path class="ear-outer" d="M21 46 Q 19 22 27 7 Q 41 16 52 29 Z" />
                        <path class="ear-inner" d="M28 37 Q 27 24 31 15 Q 39 22 45 30 Z" />
                    </g>
                    <g class="ear ear-r">
                        <path class="ear-outer" d="M99 46 Q 101 22 93 7 Q 79 16 68 29 Z" />
                        <path class="ear-inner" d="M92 37 Q 93 24 89 15 Q 81 22 75 30 Z" />
                    </g>
                    <ellipse class="head" cx="60" cy="56" rx="43" ry="35" />
                    <g class="stripes">
                        <path d="M60 22 q -2 7 0 13 q 2 -6 0 -13 z" />
                        <path d="M50 24 q -4 6 -2 11 q 3 -5 2 -11 z" />
                        <path d="M70 24 q 4 6 2 11 q -3 -5 -2 -11 z" />
                    </g>
                    <ellipse class="gloss" cx="44" cy="35" rx="18" ry="9" />
                    <ellipse class="blush" cx="30" cy="70" rx="7" ry="4" />
                    <ellipse class="blush" cx="90" cy="70" rx="7" ry="4" />
                    <ellipse class="muzzle" cx="52" cy="72" rx="10" ry="7.5" />
                    <ellipse class="muzzle" cx="68" cy="72" rx="10" ry="7.5" />
                    <g class="eyes">
                        <g class="eye-wrap">
                            <ellipse class="eye" cx="42" cy="55" rx="7.5" ry="9" />
                            <g class="pupil">
                                <circle class="shine" cx="44.6" cy="51.4" r="2.8" />
                                <circle class="shine small" cx="39.6" cy="58.6" r="1.2" />
                            </g>
                        </g>
                        <g class="eye-wrap">
                            <ellipse class="eye" cx="78" cy="55" rx="7.5" ry="9" />
                            <g class="pupil">
                                <circle class="shine" cx="80.6" cy="51.4" r="2.8" />
                                <circle class="shine small" cx="75.6" cy="58.6" r="1.2" />
                            </g>
                        </g>
                        <path class="sleep-eye" d="M34 57 Q 42 62 50 57" />
                        <path class="sleep-eye" d="M70 57 Q 78 62 86 57" />
                    </g>
                    <rect class="visor" x="24" y="45" width="72" height="20" rx="10" />
                    <rect class="visor-scan" x="27" y="48" width="11" height="14" rx="5.5" />
                    <path class="nose" d="M55.5 66.5 Q 60 64.5 64.5 66.5 Q 62.5 70.5 60 71 Q 57.5 70.5 55.5 66.5 Z" />
                    <path class="mouth" d="M60 71 q -3.5 5 -8 2.5 M60 71 q 3.5 5 8 2.5" />
                    <g class="whiskers">
                        <path d="M12 67 L 33 70 M11 75 L 33 75 M87 70 L 108 67 M87 75 L 109 75" />
                    </g>
                </g>
                <g class="zzz">
                    <text x="98" y="20">"z"</text>
                    <text x="106" y="10">"z"</text>
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
