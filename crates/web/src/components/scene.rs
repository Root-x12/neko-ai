//! The animated scenery behind the app: an anime-style lake below Mt. Fuji (Neko) or a neon
//! lakeside city at night (Cyber). Pure SVG + CSS transform animations, so it stays cheap.
//! Colours come from CSS variables (day/night per scheme, leaf colours per palette).

use leptos::prelude::*;

/// Deterministic pseudo-random numbers so the layout looks organic but never changes.
fn rnd(seed: u32) -> f64 {
    let mut x = seed.wrapping_mul(2_654_435_761).wrapping_add(0x9e37_79b9);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    f64::from(x % 10_000) / 10_000.0
}

/// Tiny sitting cat seen from behind-ish, used in the boat (origin at its feet).
const BOAT_CAT: &str = r##"<g class="boat-cat">
  <path class="bc-tail" d="M14 -6 C 26 -6 28 -18 20 -22" />
  <ellipse class="bc-fur" cx="4" cy="-10" rx="12" ry="10" />
  <path class="bc-fur" d="M-6 -24 L -5 -36 L 2 -28 Z M14 -24 L 13 -36 L 6 -28 Z" />
  <circle class="bc-fur" cx="4" cy="-24" r="9.5" />
  <path class="bc-ear" d="M-4 -26 L -3.5 -33 L 0.5 -28.5 Z M12 -26 L 11.5 -33 L 7.5 -28.5 Z" />
</g>"##;

fn neko_scene() -> String {
    let mut stars = String::new();
    for i in 0..60u32 {
        let (x, y) = (rnd(i) * 1600.0, rnd(i + 100) * 360.0);
        let r = 0.6 + rnd(i + 200) * 1.4;
        let d = rnd(i + 300) * 6.0;
        stars.push_str(&format!(
            r#"<circle class="star" cx="{x:.0}" cy="{y:.0}" r="{r:.1}" style="animation-delay:-{d:.1}s"/>"#
        ));
    }
    let mut shimmer = String::new();
    for i in 0..26u32 {
        let x = rnd(i + 400) * 1500.0;
        let y = 600.0 + rnd(i + 500) * 280.0;
        let w = 18.0 + rnd(i + 600) * 60.0 * ((y - 560.0) / 340.0 + 0.4);
        let d = rnd(i + 700) * 5.0;
        shimmer.push_str(&format!(
            r#"<rect class="shimmer" x="{x:.0}" y="{y:.0}" width="{w:.0}" height="2" rx="1" style="animation-delay:-{d:.1}s"/>"#
        ));
    }
    let mut grass = String::new();
    for i in 0..28u32 {
        let x = rnd(i + 800) * 420.0;
        let h = 22.0 + rnd(i + 900) * 34.0;
        let base = 900.0 - (x / 420.0) * 60.0;
        let d = rnd(i + 1000) * 4.0;
        grass.push_str(&format!(
            r#"<path class="blade" d="M{x:.0} {base:.0} q 4 -{h2:.0} {lean:.0} -{h:.0}" style="animation-delay:-{d:.1}s"/>"#,
            h2 = h * 0.6,
            lean = 6.0 + rnd(i + 1100) * 8.0
        ));
    }
    let mut flies = String::new();
    for i in 0..14u32 {
        let x = 200.0 + rnd(i + 1200) * 1300.0;
        let y = 560.0 + rnd(i + 1300) * 300.0;
        let d = rnd(i + 1400) * 9.0;
        flies.push_str(&format!(
            r#"<circle class="firefly" cx="{x:.0}" cy="{y:.0}" r="2.4" style="animation-delay:-{d:.1}s"/>"#
        ));
    }
    let fuji = r#"<path class="mtn" d="M470 562 L 745 300 Q 800 262 855 300 L 1130 562 Z"/>
      <path class="snow" d="M688 355 L 745 300 Q 800 262 855 300 L 912 355 Q 884 342 862 364 Q 838 344 812 368 Q 788 344 760 366 Q 738 342 714 362 Z"/>"#;
    format!(
        r##"<svg class="scene neko-scene" viewBox="0 0 1600 900" preserveAspectRatio="xMidYMax slice">
  <defs>
    <linearGradient id="n-sky" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" style="stop-color:var(--sky-top)"/><stop offset=".55" style="stop-color:var(--sky-mid)"/><stop offset="1" style="stop-color:var(--sky-bot)"/>
    </linearGradient>
    <linearGradient id="n-lake" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" style="stop-color:var(--lake-top)"/><stop offset="1" style="stop-color:var(--lake-bot)"/>
    </linearGradient>
    <radialGradient id="n-sun"><stop offset="0" style="stop-color:var(--sun)"/><stop offset=".35" style="stop-color:var(--sun)"/><stop offset="1" style="stop-color:var(--sun)" stop-opacity="0"/></radialGradient>
  </defs>
  <rect width="1600" height="900" fill="url(#n-sky)"/>
  <g class="stars">{stars}</g>
  <circle class="sun-glow" cx="1180" cy="230" r="170" fill="url(#n-sun)"/>
  <circle class="sun" cx="1180" cy="230" r="52"/>
  <g class="clouds">
    <g class="cloud c1"><ellipse cx="200" cy="150" rx="90" ry="26"/><ellipse cx="250" cy="134" rx="60" ry="30"/><ellipse cx="150" cy="140" rx="50" ry="22"/></g>
    <g class="cloud c2"><ellipse cx="900" cy="110" rx="120" ry="24"/><ellipse cx="960" cy="94" rx="70" ry="28"/><ellipse cx="840" cy="100" rx="55" ry="20"/></g>
    <g class="cloud c3"><ellipse cx="1400" cy="330" rx="80" ry="18"/><ellipse cx="1440" cy="318" rx="46" ry="20"/></g>
  </g>
  {fuji}
  <path class="hill-far" d="M0 562 Q 170 488 380 540 Q 520 572 640 550 L 640 562 Z M960 562 Q 1120 520 1300 528 Q 1460 534 1600 500 L 1600 562 Z"/>
  <rect y="560" width="1600" height="340" fill="url(#n-lake)"/>
  <g class="reflection" transform="translate(0 1122) scale(1 -1)">{fuji}</g>
  <g class="shimmers">{shimmer}</g>
  <g class="boat-wrap"><g class="boat">
    <ellipse class="ripple" cx="0" cy="4" rx="80" ry="5"/>
    <path class="hull" d="M-66 -6 Q 0 20 66 -6 L 58 -16 L -58 -16 Z"/>
    <path class="hull-rim" d="M-58 -16 L 58 -16"/>
    <g transform="translate(-8 -16)">{BOAT_CAT}</g>
    <path class="mast" d="M-40 -16 L -40 -58"/>
    <rect class="lantern" x="-47" y="-58" width="14" height="18" rx="5"/>
  </g></g>
  <path class="shore" d="M0 900 L 0 700 Q 140 668 270 700 Q 380 730 470 900 Z"/>
  <g class="torii">
    <rect x="150" y="598" width="13" height="104" rx="2"/><rect x="232" y="598" width="13" height="104" rx="2"/>
    <path d="M122 592 Q 197 578 272 592 L 276 606 Q 197 594 118 606 Z"/>
    <rect x="138" y="618" width="120" height="9" rx="2"/>
    <rect x="192" y="604" width="12" height="16"/>
  </g>
  <g class="grass">{grass}</g>
  <path class="shore shore-r" d="M1600 900 L 1600 690 Q 1470 676 1370 716 Q 1290 750 1250 900 Z"/>
  <g class="tree">
    <path class="trunk" d="M1500 760 C 1494 690 1478 640 1440 590 M1470 660 C 1440 640 1400 640 1370 620"/>
    <g class="foliage">
      <circle cx="1420" cy="560" r="72"/><circle cx="1500" cy="530" r="82"/><circle cx="1350" cy="600" r="54"/>
      <circle cx="1570" cy="585" r="62"/><circle cx="1455" cy="480" r="58"/>
    </g>
    <g class="foliage-hi"><circle cx="1400" cy="530" r="40"/><circle cx="1490" cy="490" r="44"/></g>
  </g>
  <g class="fireflies">{flies}</g>
</svg>"##
    )
}

fn cyber_scene() -> String {
    // Skyline along the horizon (y = 560), with lit windows.
    let mut city = String::new();
    let mut windows = String::new();
    let mut x: f64 = 0.0;
    let mut i = 0u32;
    while x < 1600.0 {
        let w = 38.0 + rnd(i + 2000) * 70.0;
        // Low skyline so it sits behind the content instead of competing with it.
        let h = 36.0 + rnd(i + 2100) * 120.0 * (1.0 - ((x - 800.0).abs() / 1800.0));
        let top = 560.0 - h;
        city.push_str(&format!(r#"<rect x="{x:.0}" y="{top:.0}" width="{w:.0}" height="{h:.0}"/>"#));
        if rnd(i + 2200) > 0.6 {
            city.push_str(&format!(
                r#"<rect class="antenna" x="{ax:.0}" y="{ay:.0}" width="2" height="24"/><circle class="beacon" cx="{bx:.0}" cy="{ay:.0}" r="2.5" style="animation-delay:-{d:.1}s"/>"#,
                ax = x + w / 2.0,
                ay = top - 24.0,
                bx = x + w / 2.0 + 1.0,
                d = rnd(i + 2300) * 3.0
            ));
        }
        let mut wy = top + 10.0;
        let mut row = 0u32;
        while wy < 548.0 {
            let mut wx = x + 6.0;
            let mut col = 0u32;
            while wx < x + w - 8.0 {
                let seed = i * 1000 + row * 31 + col;
                if rnd(seed + 3000) > 0.62 {
                    let cls = if rnd(seed + 4000) > 0.92 { "win twinkle" } else { "win" };
                    let alt = if rnd(seed + 5000) > 0.5 { " alt" } else { "" };
                    windows.push_str(&format!(
                        r#"<rect class="{cls}{alt}" x="{wx:.0}" y="{wy:.0}" width="5" height="7" style="animation-delay:-{d:.1}s"/>"#,
                        d = rnd(seed + 6000) * 6.0
                    ));
                }
                wx += 11.0;
                col += 1;
            }
            wy += 16.0;
            row += 1;
        }
        x += w + 4.0 + rnd(i + 2400) * 10.0;
        i += 1;
    }
    let mut stars = String::new();
    for i in 0..40u32 {
        let (x, y) = (rnd(i + 7000) * 1600.0, rnd(i + 7100) * 300.0);
        let d = rnd(i + 7200) * 6.0;
        stars.push_str(&format!(
            r#"<circle class="star" cx="{x:.0}" cy="{y:.0}" r="1" style="animation-delay:-{d:.1}s"/>"#
        ));
    }
    let mut ripples = String::new();
    for i in 0..18u32 {
        let y = 572.0 + rnd(i + 8000) * 310.0;
        let x = rnd(i + 8100) * 1450.0;
        let w = 40.0 + rnd(i + 8200) * 160.0;
        let d = rnd(i + 8300) * 8.0;
        ripples.push_str(&format!(
            r#"<rect class="scan" x="{x:.0}" y="{y:.0}" width="{w:.0}" height="1.5" style="animation-delay:-{d:.1}s"/>"#
        ));
    }
    format!(
        r##"<svg class="scene cyber-scene" viewBox="0 0 1600 900" preserveAspectRatio="xMidYMax slice">
  <defs>
    <linearGradient id="c-sky" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" style="stop-color:var(--cy-sky-top)"/><stop offset=".7" style="stop-color:var(--cy-sky-mid)"/><stop offset="1" style="stop-color:var(--cy-sky-bot)"/>
    </linearGradient>
    <linearGradient id="c-sun" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" style="stop-color:var(--accent-2)"/><stop offset="1" style="stop-color:var(--accent)"/>
    </linearGradient>
    <linearGradient id="c-lake" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" style="stop-color:var(--cy-lake-top)"/><stop offset="1" style="stop-color:var(--cy-lake-bot)"/>
    </linearGradient>
    <mask id="c-stripes">
      <rect x="600" y="260" width="400" height="320" fill="#fff"/>
      <rect x="600" y="470" width="400" height="7" fill="#000"/><rect x="600" y="492" width="400" height="9" fill="#000"/>
      <rect x="600" y="516" width="400" height="11" fill="#000"/><rect x="600" y="541" width="400" height="13" fill="#000"/>
    </mask>
  </defs>
  <rect width="1600" height="900" fill="url(#c-sky)"/>
  <g class="stars">{stars}</g>
  <circle class="c-sun" cx="800" cy="420" r="150" fill="url(#c-sun)" mask="url(#c-stripes)"/>
  <g class="city">{city}</g>
  <g class="windows">{windows}</g>
  <rect y="560" width="1600" height="340" fill="url(#c-lake)"/>
  <g class="c-reflection" transform="translate(0 1120) scale(1 -1)">
    <circle cx="800" cy="420" r="150" fill="url(#c-sun)" mask="url(#c-stripes)"/>
    <g class="city">{city}</g><g class="windows">{windows}</g>
  </g>
  <rect class="horizon" y="559" width="1600" height="2"/>
  <g class="scans">{ripples}</g>
  <g class="boat-wrap"><g class="boat">
    <ellipse class="ripple" cx="0" cy="4" rx="80" ry="4"/>
    <path class="hull" d="M-66 -6 Q 0 18 66 -6 L 58 -16 L -58 -16 Z"/>
    <path class="mast" d="M30 -16 L 30 -54"/>
    <circle class="lamp" cx="30" cy="-56" r="5"/>
  </g></g>
  <path class="pier" d="M300 700 L 860 700 L 860 712 L 300 712 Z"/>
  <path class="pier-edge" d="M300 700 L 860 700"/>
  <g class="posts"><rect x="330" y="712" width="8" height="90"/><rect x="500" y="712" width="8" height="90"/><rect x="670" y="712" width="8" height="90"/><rect x="840" y="712" width="8" height="90"/></g>
  <g class="walker"><g class="walker-flip"><g class="walk-cat">
    <path class="wc-tail" d="M-18 -18 C -32 -22 -34 -38 -26 -44"/>
    <ellipse class="wc-body" cx="0" cy="-16" rx="20" ry="10"/>
    <circle class="wc-head" cx="20" cy="-28" r="10"/>
    <path class="wc-ear" d="M13 -35 L 14 -46 L 20 -38 Z M23 -37 L 28 -46 L 29 -34 Z"/>
    <rect class="wc-visor" x="18" y="-31" width="12" height="4" rx="2"/>
    <path class="leg l1" d="M-12 -8 L -14 0"/><path class="leg l2" d="M-6 -8 L -4 0"/>
    <path class="leg l1" d="M8 -8 L 10 0"/><path class="leg l2" d="M14 -8 L 12 0"/>
  </g></g></g>
  <path class="shore-r" d="M1600 900 L 1600 720 Q 1480 706 1390 740 Q 1320 770 1290 900 Z"/>
</svg>"##
    )
}

/// Leaves (Neko) or glowing particles (Cyber) carried across the screen by the wind.
fn wind_items(class: &'static str, count: u32, seed: u32) -> impl IntoView {
    (0..count)
        .map(|i| {
            let top = 4.0 + rnd(seed + i) * 70.0;
            let delay = rnd(seed + i + 50) * 30.0;
            let dur = 18.0 + rnd(seed + i + 100) * 16.0;
            let size = 9.0 + rnd(seed + i + 150) * 10.0;
            let flutter = 1.2 + rnd(seed + i + 200) * 1.4;
            let alt = if i % 3 == 0 { " alt" } else { "" };
            let style = format!(
                "top:{top:.1}%;animation-delay:-{delay:.1}s;animation-duration:{dur:.1}s;--s:{size:.1}px;--f:{flutter:.2}s"
            );
            view! {
                <span class="wind" style=style>
                    <i class=format!("{class}{alt}")></i>
                </span>
            }
        })
        .collect_view()
}

#[component]
pub fn Backdrop() -> impl IntoView {
    view! {
        <div class="backdrop" aria-hidden="true">
            <div class="neko-bg">
                <div class="scene-host" inner_html=neko_scene()></div>
                <div class="winds">{wind_items("leaf", 16, 10)}</div>
            </div>
            <div class="cyber-bg">
                <div class="scene-host" inner_html=cyber_scene()></div>
                <div class="winds">{wind_items("spark", 18, 90)}</div>
                <div class="scanlines"></div>
            </div>
            <div class="veil"></div>
        </div>
    }
}
