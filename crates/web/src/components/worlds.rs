//! One anime-style world per palette. Each is a layered SVG (far → near) whose layers shift a
//! little with the pointer for depth; motion is CSS transform/opacity animation only.
//! Colours that change between day and night are CSS variables (see `main.css`, "Worlds").

use std::fmt::Write;

fn rnd(seed: u32) -> f64 {
    let mut x = seed.wrapping_mul(2_654_435_761).wrapping_add(0x9e37_79b9);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    f64::from(x % 10_000) / 10_000.0
}

/// Viewport-filling SVG wrapper anchored at the bottom centre.
fn svg(class: &str, defs: &str, body: &str) -> String {
    format!(
        r##"<svg class="scene {class}" viewBox="0 0 1600 900" preserveAspectRatio="xMidYMax slice"><defs>{defs}</defs>{body}</svg>"##
    )
}

fn sky_gradient(id: &str, a: &str, b: &str, c: &str) -> String {
    format!(
        r##"<linearGradient id="{id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" style="stop-color:var({a})"/><stop offset=".55" style="stop-color:var({b})"/><stop offset="1" style="stop-color:var({c})"/></linearGradient>"##
    )
}

fn stars(seed: u32, n: u32, ymax: f64) -> String {
    let mut s = String::from(r##"<g class="w-stars">"##);
    for i in 0..n {
        let (x, y) = (rnd(seed + i) * 1600.0, rnd(seed + i + 500) * ymax);
        let r = 0.5 + rnd(seed + i + 900) * 1.5;
        let d = rnd(seed + i + 1300) * 6.0;
        let _ = write!(s, r##"<circle cx="{x:.0}" cy="{y:.0}" r="{r:.1}" style="animation-delay:-{d:.1}s"/>"##);
    }
    s.push_str("</g>");
    s
}

/// A puffy anime cloud: overlapping circles with a flat-ish base, plus a lighter top.
fn cloud(x: f64, y: f64, w: f64, seed: u32) -> String {
    let mut s = String::from(r##"<g class="w-cloud">"##);
    let n = 5 + (rnd(seed) * 3.0) as u32;
    for i in 0..n {
        let t = i as f64 / (n - 1) as f64;
        let cx = x + (t - 0.5) * w;
        let r = w * (0.16 + 0.14 * (1.0 - (t - 0.5).abs() * 2.0)) * (0.8 + rnd(seed + i) * 0.4);
        let _ = write!(s, r##"<circle cx="{cx:.0}" cy="{cy:.0}" r="{r:.0}"/>"##, cy = y - r * 0.4);
    }
    let _ = write!(
        s,
        r##"<rect x="{x0:.0}" y="{y0:.0}" width="{w:.0}" height="{h:.0}" rx="{rx:.0}"/>"##,
        x0 = x - w * 0.55,
        y0 = y - w * 0.08,
        w = w * 1.1,
        h = w * 0.16,
        rx = w * 0.08
    );
    for i in 0..3 {
        let cx = x - w * 0.15 + i as f64 * w * 0.15;
        let r = w * 0.09;
        let _ = write!(s, r##"<circle class="hi" cx="{cx:.0}" cy="{cy:.0}" r="{r:.0}"/>"##, cy = y - w * 0.22);
    }
    s.push_str("</g>");
    s
}

pub fn cloud_row(seed: u32, y: f64, count: u32, size: f64) -> String {
    let mut s = String::new();
    for i in 0..count {
        let x = -100.0 + (i as f64 + rnd(seed + i) * 0.6) * (1900.0 / count as f64);
        let w = size * (0.7 + rnd(seed + i + 40) * 0.7);
        s.push_str(&cloud(x, y + rnd(seed + i + 80) * 30.0, w, seed + i * 7));
    }
    s
}

/// A flock flying across the sky; each bird flaps on its own beat.
pub fn birds(seed: u32, n: u32, y: f64) -> String {
    let mut s = String::from(r##"<g class="w-flock">"##);
    for i in 0..n {
        let x = rnd(seed + i) * 260.0;
        let by = y + rnd(seed + i + 30) * 120.0 - 60.0;
        let sc = 0.6 + rnd(seed + i + 60) * 0.7;
        let d = rnd(seed + i + 90) * 0.8;
        let _ = write!(
            s,
            r##"<g transform="translate({x:.0} {by:.0}) scale({sc:.2})"><path class="w-bird" style="animation-delay:-{d:.2}s" d="M-12 0 Q -6 -7 0 0 Q 6 -7 12 0"/></g>"##
        );
    }
    s.push_str("</g>");
    s
}

/// A sitting cat seen from behind (origin at its feet), optionally with a scarf in the wind.
fn sitting_cat(x: f64, y: f64, scale: f64, scarf: bool) -> String {
    let scarf_svg = if scarf {
        r##"<path class="w-scarf" d="M-15 -52 Q 0 -44 15 -52 L 14 -46 Q 0 -38 -14 -46 Z"/>
           <path class="w-scarf w-scarf-tail" d="M12 -50 C 30 -54 44 -46 60 -54 C 52 -44 36 -40 14 -44 Z"/>"##
    } else {
        ""
    };
    format!(
        r##"<g class="w-cat" transform="translate({x:.0} {y:.0}) scale({scale:.2})">
  <path class="w-cat-tail" d="M16 -4 C 40 -2 44 -22 32 -32"/>
  <path class="w-cat-body" d="M-22 0 Q -26 -36 -12 -50 Q 0 -56 12 -50 Q 26 -36 22 0 Z"/>
  <circle class="w-cat-body" cx="0" cy="-62" r="15"/>
  <path class="w-cat-body" d="M-14 -66 L -12 -84 L -3 -74 Z M14 -66 L 12 -84 L 3 -74 Z"/>
  <path class="w-cat-rim" d="M-15 -60 Q -14 -74 -4 -77 M-22 -2 Q -25 -34 -13 -48"/>
  {scarf_svg}
</g>"##
    )
}

fn torii(x: f64, y: f64, w: f64, h: f64) -> String {
    let p = w * 0.09;
    format!(
        r##"<g class="w-torii">
  <rect x="{l:.0}" y="{top:.0}" width="{p:.0}" height="{h:.0}"/><rect x="{r:.0}" y="{top:.0}" width="{p:.0}" height="{h:.0}"/>
  <path d="M{k0:.0} {ky:.0} Q {x:.0} {kq:.0} {k1:.0} {ky:.0} L {k1b:.0} {kyb:.0} Q {x:.0} {kqb:.0} {k0b:.0} {kyb:.0} Z"/>
  <rect class="w-torii-dark" x="{n0:.0}" y="{ny:.0}" width="{nw:.0}" height="{nh:.0}"/>
  <rect x="{c0:.0}" y="{cy:.0}" width="{cw:.0}" height="{ch:.0}"/>
  <rect class="w-torii-dark" x="{l:.0}" y="{base:.0}" width="{p:.0}" height="{bh:.0}"/><rect class="w-torii-dark" x="{r:.0}" y="{base:.0}" width="{p:.0}" height="{bh:.0}"/>
</g>"##,
        l = x - w * 0.38,
        r = x + w * 0.38 - p,
        top = y - h,
        k0 = x - w * 0.6,
        k1 = x + w * 0.6,
        ky = y - h - w * 0.02,
        kq = y - h - w * 0.12,
        k0b = x - w * 0.62,
        k1b = x + w * 0.62,
        kyb = y - h + w * 0.05,
        kqb = y - h - w * 0.03,
        n0 = x - w * 0.48,
        ny = y - h + w * 0.15,
        nw = w * 0.96,
        nh = w * 0.06,
        c0 = x - p * 0.4,
        cy = y - h + w * 0.05,
        cw = p * 0.8,
        ch = w * 0.11,
        base = y - h * 0.08,
        bh = h * 0.08
    )
}

fn lantern(x: f64, y: f64, s: f64) -> String {
    format!(
        r##"<g class="w-toro" transform="translate({x:.0} {y:.0}) scale({s:.2})">
  <rect x="-14" y="-6" width="28" height="6" rx="2"/><rect x="-5" y="-40" width="10" height="34"/>
  <rect x="-16" y="-46" width="32" height="6" rx="2"/><rect class="w-light" x="-11" y="-68" width="22" height="22" rx="3"/>
  <path d="M-22 -68 Q 0 -86 22 -68 Z"/><circle cx="0" cy="-82" r="3"/>
</g>"##
    )
}

// ---------- Sakura: shrine above the clouds ----------

pub fn sakura() -> String {
    let defs = format!(
        r##"{}<linearGradient id="sk-ray" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".45"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></linearGradient>
        <radialGradient id="sk-sun"><stop offset="0" style="stop-color:var(--w-sun)"/><stop offset=".3" style="stop-color:var(--w-sun)"/><stop offset="1" style="stop-color:var(--w-sun)" stop-opacity="0"/></radialGradient>"##,
        sky_gradient("sk-sky", "--w-sky1", "--w-sky2", "--w-sky3")
    );
    let mut pagoda = String::new();
    let (cx, mut ty) = (560.0, 560.0);
    for i in 0..5 {
        let w = 120.0 - i as f64 * 18.0;
        let _ = write!(
            pagoda,
            r##"<rect class="w-wall" x="{x:.0}" y="{y:.0}" width="{ww:.0}" height="34"/>
               <rect class="w-window" x="{wx:.0}" y="{wy:.0}" width="{wiw:.0}" height="12" rx="2"/>
               <path class="w-roof" d="M{a:.0} {ry:.0} Q {cx:.0} {rq:.0} {b:.0} {ry:.0} L {b2:.0} {ry2:.0} L {a2:.0} {ry2:.0} Z"/>
               <path class="w-roof-edge" d="M{a:.0} {ry:.0} Q {cx:.0} {rq:.0} {b:.0} {ry:.0}"/>"##,
            x = cx - w * 0.62,
            y = ty - 34.0,
            ww = w * 1.24,
            wx = cx - w * 0.4,
            wy = ty - 26.0,
            wiw = w * 0.8,
            a = cx - w,
            b = cx + w,
            ry = ty - 34.0,
            rq = ty - 58.0,
            b2 = cx + w * 0.8,
            a2 = cx - w * 0.8,
            ry2 = ty - 24.0
        );
        ty -= 46.0;
    }
    let _ = write!(
        pagoda,
        r##"<path class="w-roof" d="M{x0:.0} {y:.0} L {cx:.0} {top:.0} L {x1:.0} {y:.0} Z"/>"##,
        x0 = cx - 14.0,
        x1 = cx + 14.0,
        y = ty - 10.0,
        top = ty - 70.0
    );
    let mut blossoms = String::new();
    for i in 0..22u32 {
        let x = 1420.0 + rnd(i + 3000) * 220.0;
        let y = 260.0 + rnd(i + 3100) * 170.0;
        let r = 26.0 + rnd(i + 3200) * 30.0;
        let cls = if i % 3 == 0 { "w-bloom hi" } else { "w-bloom" };
        let _ = write!(blossoms, r##"<circle class="{cls}" cx="{x:.0}" cy="{y:.0}" r="{r:.0}"/>"##);
    }
    let body = format!(
        r##"<rect width="1600" height="900" fill="url(#sk-sky)"/>
{stars}
<circle cx="1040" cy="150" r="190" fill="url(#sk-sun)" class="w-glow"/><circle class="w-sun" cx="1040" cy="150" r="44"/>
<g class="w-rays"><path d="M180 -40 L 360 -40 L 760 900 L 420 900 Z"/><path d="M520 -40 L 600 -40 L 980 900 L 820 900 Z"/><path d="M-80 -40 L 40 -40 L 300 900 L 80 900 Z"/></g>
<g class="w-layer far"><path class="w-peak" d="M-40 560 L 140 400 L 260 470 L 360 380 L 520 560 Z M880 560 L 1020 420 L 1120 490 L 1260 360 L 1460 560 Z"/></g>
<g class="w-clouds drift-a w-layer far">{c1}</g>
<g class="w-layer mid">
  <path class="w-rock" d="M370 600 Q 560 520 770 600 Q 720 690 600 730 Q 520 745 460 700 Q 400 660 370 600 Z"/>
  <path class="w-moss" d="M380 598 Q 560 530 760 598 Q 560 572 380 598 Z"/>
  {pagoda}
  {small_torii}
</g>
<g class="w-clouds drift-b w-layer mid">{c2}</g>
<g class="w-flock-path">{flock}</g>
<g class="w-layer near">
  <path class="w-cliff" d="M1010 900 Q 1060 760 1150 668 Q 1230 640 1330 650 Q 1460 640 1600 610 L 1600 900 Z"/>
  <path class="w-cliff-hi" d="M1150 668 Q 1230 640 1330 650 Q 1460 640 1600 610 L 1600 626 Q 1460 656 1330 664 Q 1230 656 1156 682 Z"/>
  <path class="w-path" d="M1180 668 Q 1300 660 1420 650 L 1450 680 Q 1310 694 1200 700 Z"/>
  {big_torii}
  {lantern1}{lantern2}
  <path class="w-trunk" d="M1560 640 C 1550 520 1530 440 1470 370 M1540 520 C 1500 470 1460 450 1420 440"/>
  <g class="w-tree">{blossoms}</g>
  {cat}
</g>
<g class="w-clouds drift-c w-layer near">{c3}</g>"##,
        stars = stars(10, 70, 380.0),
        c1 = cloud_row(100, 560.0, 9, 260.0),
        c2 = cloud_row(200, 690.0, 7, 340.0),
        c3 = cloud_row(300, 880.0, 5, 420.0),
        pagoda = pagoda,
        small_torii = torii(700.0, 600.0, 60.0, 70.0),
        flock = birds(400, 9, 230.0),
        big_torii = torii(1330.0, 656.0, 260.0, 300.0),
        lantern1 = lantern(1200.0, 680.0, 0.9),
        lantern2 = lantern(1470.0, 650.0, 0.9),
        cat = sitting_cat(1110.0, 690.0, 1.0, true),
    );
    svg("w-sakura", &defs, &body)
}

// ---------- Matcha: bamboo forest ----------

fn bamboo(seed: u32, n: u32, cls: &str, wmin: f64, wmax: f64) -> String {
    let mut s = format!(r##"<g class="w-bamboo {cls}">"##);
    for i in 0..n {
        let x = rnd(seed + i) * 1640.0 - 20.0;
        let w = wmin + rnd(seed + i + 50) * (wmax - wmin);
        let lean = (rnd(seed + i + 100) - 0.5) * 40.0;
        let d = rnd(seed + i + 150) * 6.0;
        let _ = write!(
            s,
            r##"<g class="w-stalk" style="animation-delay:-{d:.1}s"><path d="M{x0:.0} 920 L {x1:.0} -20 L {x2:.0} -20 L {x3:.0} 920 Z"/>"##,
            x0 = x,
            x1 = x + lean,
            x2 = x + lean + w,
            x3 = x + w
        );
        let mut y = 60.0 + rnd(seed + i + 200) * 80.0;
        while y < 900.0 {
            let t = y / 900.0;
            let nx = x + lean * (1.0 - t);
            let _ = write!(s, r##"<rect class="w-node" x="{nx:.0}" y="{y:.0}" width="{w:.0}" height="4" rx="2"/>"##);
            if rnd(seed + i + (y as u32)) > 0.7 {
                let dir = if rnd(seed + i + (y as u32) + 7) > 0.5 { 1.0 } else { -1.0 };
                let lx = nx + if dir > 0.0 { w } else { 0.0 };
                let _ = write!(
                    s,
                    r##"<path class="w-bleaf" d="M{lx:.0} {y:.0} q {a:.0} -14 {b:.0} -6 q {c:.0} 4 {nb:.0} 6 z"/>"##,
                    a = dir * 30.0,
                    b = dir * 64.0,
                    c = -dir * 30.0,
                    nb = -dir * 64.0
                );
            }
            y += 110.0 + rnd(seed + i + (y as u32) + 3) * 60.0;
        }
        s.push_str("</g>");
    }
    s.push_str("</g>");
    s
}

pub fn matcha() -> String {
    let defs = format!(
        r##"{}<linearGradient id="mt-ray" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".5"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></linearGradient>"##,
        sky_gradient("mt-sky", "--w-sky1", "--w-sky2", "--w-sky3")
    );
    let mut stones = String::new();
    for i in 0..9u32 {
        let t = i as f64 / 8.0;
        let x = 800.0 + (t * 3.0).sin() * 120.0 * (1.0 - t) + (t - 0.5) * 60.0;
        let y = 900.0 - t * 230.0;
        let rx = 70.0 * (1.0 - t * 0.75);
        let _ = write!(
            stones,
            r##"<ellipse class="w-stone" cx="{x:.0}" cy="{y:.0}" rx="{rx:.0}" ry="{ry:.0}"/>"##,
            ry = rx * 0.32
        );
    }
    let mut flies = String::from(r##"<g class="w-fireflies">"##);
    for i in 0..18u32 {
        let _ = write!(
            flies,
            r##"<circle cx="{x:.0}" cy="{y:.0}" r="2.6" style="animation-delay:-{d:.1}s"/>"##,
            x = 200.0 + rnd(i + 4000) * 1200.0,
            y = 420.0 + rnd(i + 4100) * 420.0,
            d = rnd(i + 4200) * 8.0
        );
    }
    flies.push_str("</g>");
    let body = format!(
        r##"<rect width="1600" height="900" fill="url(#mt-sky)"/>
<g class="w-rays w-rays-diag"><path d="M300 -50 L 520 -50 L 1100 900 L 760 900 Z"/><path d="M700 -50 L 800 -50 L 1300 900 L 1150 900 Z"/><path d="M1000 -50 L 1060 -50 L 1500 900 L 1400 900 Z"/></g>
<g class="w-layer far">{far}</g>
<g class="w-mist drift-a"><ellipse cx="400" cy="640" rx="520" ry="60"/><ellipse cx="1300" cy="600" rx="460" ry="50"/></g>
<g class="w-layer mid">{mid}</g>
<path class="w-ground" d="M0 900 L 0 700 Q 400 650 800 676 Q 1200 650 1600 700 L 1600 900 Z"/>
<g class="w-layer near">
  {stones}
  {l1}{l2}
  {cat}
</g>
<g class="w-layer near">{near}</g>
<g class="w-mist drift-b"><ellipse cx="900" cy="800" rx="700" ry="70"/></g>
{flies}"##,
        far = bamboo(5000, 26, "far", 10.0, 18.0),
        mid = bamboo(6000, 16, "mid", 18.0, 30.0),
        near = bamboo(7000, 6, "near", 34.0, 52.0),
        stones = stones,
        l1 = lantern(560.0, 740.0, 1.3),
        l2 = lantern(1080.0, 720.0, 1.2),
        cat = sitting_cat(900.0, 760.0, 0.8, false),
        flies = flies,
    );
    svg("w-matcha", &defs, &body)
}

// ---------- Mochi: sunset town ----------

pub fn mochi() -> String {
    let defs = format!(
        r##"{}<radialGradient id="mc-sun"><stop offset="0" style="stop-color:var(--w-sun)"/><stop offset=".35" style="stop-color:var(--w-sun)"/><stop offset="1" style="stop-color:var(--w-sun)" stop-opacity="0"/></radialGradient>"##,
        sky_gradient("mc-sky", "--w-sky1", "--w-sky2", "--w-sky3")
    );
    let mut streaks = String::new();
    for i in 0..8u32 {
        let _ = write!(
            streaks,
            r##"<rect class="w-streak" x="{x:.0}" y="{y:.0}" width="{w:.0}" height="{h:.0}" rx="{r:.0}"/>"##,
            x = rnd(i + 5100) * 1500.0 - 100.0,
            y = 120.0 + rnd(i + 5200) * 300.0,
            w = 240.0 + rnd(i + 5300) * 380.0,
            h = 10.0 + rnd(i + 5400) * 16.0,
            r = 12.0
        );
    }
    let houses = |seed: u32, base: f64, hmin: f64, hmax: f64, cls: &str| {
        let mut s = format!(r##"<g class="w-town {cls}">"##);
        let mut x = -20.0;
        let mut i = 0u32;
        while x < 1640.0 {
            let w = 90.0 + rnd(seed + i) * 110.0;
            let h = hmin + rnd(seed + i + 50) * (hmax - hmin);
            let top = base - h;
            let _ = write!(
                s,
                r##"<rect x="{x:.0}" y="{top:.0}" width="{w:.0}" height="{h:.0}"/><path d="M{a:.0} {top:.0} Q {m:.0} {pk:.0} {b:.0} {top:.0} Z"/>"##,
                a = x - 12.0,
                b = x + w + 12.0,
                m = x + w / 2.0,
                pk = top - 34.0 - rnd(seed + i + 70) * 20.0
            );
            let mut wy = top + 16.0;
            while wy < base - 20.0 {
                let mut wx = x + 12.0;
                while wx < x + w - 20.0 {
                    if rnd(seed + i + (wx as u32) * 3 + (wy as u32)) > 0.55 {
                        let _ = write!(
                            s,
                            r##"<rect class="w-lit" x="{wx:.0}" y="{wy:.0}" width="12" height="10" rx="1"/>"##
                        );
                    }
                    wx += 24.0;
                }
                wy += 26.0;
            }
            x += w + 6.0;
            i += 1;
        }
        s.push_str("</g>");
        s
    };
    let mut lanterns = String::from(r##"<g class="w-string"><path class="w-wire" d="M-20 90 Q 800 260 1620 90"/>"##);
    for i in 0..9u32 {
        let x = 120.0 + i as f64 * 170.0;
        let t = x / 1600.0;
        let y = 90.0 + 4.0 * 170.0 * t * (1.0 - t) + 12.0;
        let d = rnd(i + 5600) * 3.0;
        let _ = write!(
            lanterns,
            r##"<g class="w-chochin" style="animation-delay:-{d:.1}s" transform="translate({x:.0} {y:.0})"><path class="w-wire" d="M0 -12 L 0 0"/><rect class="w-cap" x="-9" y="0" width="18" height="5"/><ellipse class="w-paper" cx="0" cy="22" rx="16" ry="20"/><path class="w-rib" d="M-15 16 Q 0 20 15 16 M-15 28 Q 0 32 15 28"/><rect class="w-cap" x="-9" y="40" width="18" height="5"/></g>"##
        );
    }
    lanterns.push_str("</g>");
    let body = format!(
        r##"<rect width="1600" height="900" fill="url(#mc-sky)"/>
{stars}
<g class="w-layer far"><circle cx="820" cy="470" r="260" fill="url(#mc-sun)" class="w-glow"/><circle class="w-sun" cx="820" cy="470" r="96"/></g>
<g class="w-streaks drift-a">{streaks}</g>
<g class="w-flock-path">{flock}</g>
<g class="w-layer far">{far}</g>
<g class="w-layer mid">{mid}</g>
<g class="w-layer near">
  <path class="w-pole" d="M240 900 L 252 360 M200 400 L 300 396 M210 430 L 290 428 M1380 900 L 1366 380 M1330 420 L 1420 418 M1340 448 L 1410 446"/>
  <path class="w-wire" d="M250 400 Q 820 520 1370 420 M252 430 Q 820 560 1368 448"/>
  <g class="w-perch"><path class="w-bird-sit" d="M560 452 q 6 -10 12 0 z M600 460 q 6 -10 12 0 z M980 470 q 6 -10 12 0 z"/></g>
  {near}
  {cat}
</g>
{lanterns}"##,
        stars = stars(5000, 50, 360.0),
        streaks = streaks,
        flock = birds(5700, 7, 300.0),
        far = houses(5800, 640.0, 60.0, 130.0, "far"),
        mid = houses(5900, 760.0, 80.0, 170.0, "mid"),
        near = houses(6100, 920.0, 90.0, 150.0, "near"),
        cat = sitting_cat(700.0, 776.0, 0.9, false),
        lanterns = lanterns,
    );
    svg("w-mochi", &defs, &body)
}

// ---------- Lavender: starry hill ----------

pub fn lavender() -> String {
    let defs = format!(
        r##"{}<radialGradient id="lv-milk"><stop offset="0" stop-color="#fff" stop-opacity=".35"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></radialGradient>
        <linearGradient id="lv-shoot" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#fff" stop-opacity="0"/><stop offset="1" stop-color="#fff"/></linearGradient>"##,
        sky_gradient("lv-sky", "--w-sky1", "--w-sky2", "--w-sky3")
    );
    let mut milky = String::new();
    for i in 0..160u32 {
        let t = rnd(i + 6200);
        let x = t * 1700.0 - 50.0;
        let y = 520.0 - t * 460.0 + (rnd(i + 6300) - 0.5) * 140.0;
        let _ = write!(milky, r##"<circle cx="{x:.0}" cy="{y:.0}" r="{r:.1}"/>"##, r = 0.5 + rnd(i + 6400) * 1.1);
    }
    let mut rows = String::new();
    for i in 0..9u32 {
        let y = 720.0 + i as f64 * 24.0;
        let _ = write!(
            rows,
            r##"<path class="w-row" d="M-20 {y:.0} Q 800 {q:.0} 1620 {y2:.0}"/>"##,
            q = y - 60.0 + i as f64 * 6.0,
            y2 = y + 10.0
        );
    }
    let mut flies = String::from(r##"<g class="w-fireflies">"##);
    for i in 0..16u32 {
        let _ = write!(
            flies,
            r##"<circle cx="{x:.0}" cy="{y:.0}" r="2.4" style="animation-delay:-{d:.1}s"/>"##,
            x = 300.0 + rnd(i + 6600) * 1000.0,
            y = 560.0 + rnd(i + 6700) * 260.0,
            d = rnd(i + 6800) * 8.0
        );
    }
    flies.push_str("</g>");
    let body = format!(
        r##"<rect width="1600" height="900" fill="url(#lv-sky)"/>
<ellipse cx="800" cy="300" rx="900" ry="120" fill="url(#lv-milk)" transform="rotate(-24 800 300)" class="w-milky"/>
<g class="w-milky-stars">{milky}</g>
{stars}
<g class="w-shooting"><rect class="w-shoot s1" x="0" y="0" width="160" height="2" fill="url(#lv-shoot)"/><rect class="w-shoot s2" x="0" y="0" width="120" height="2" fill="url(#lv-shoot)"/></g>
<g class="w-layer far"><path class="w-moon" d="M1260 170 A 56 56 0 1 0 1316 250 A 44 44 0 1 1 1260 170 Z"/></g>
<g class="w-layer far"><path class="w-hill-far" d="M0 640 Q 300 560 640 610 Q 1000 660 1300 590 Q 1460 560 1600 590 L 1600 900 L 0 900 Z"/></g>
<g class="w-layer mid">
  <path class="w-hill" d="M0 720 Q 380 600 820 650 Q 1180 690 1600 640 L 1600 900 L 0 900 Z"/>
  {small_torii}
  <path class="w-trunk" d="M1180 668 C 1176 600 1170 560 1150 520 M1172 600 C 1150 580 1120 578 1100 566"/>
  <g class="w-tree"><circle class="w-canopy" cx="1150" cy="500" r="56"/><circle class="w-canopy" cx="1200" cy="520" r="44"/><circle class="w-canopy" cx="1110" cy="530" r="40"/></g>
  {cat}
</g>
<g class="w-layer near"><path class="w-field" d="M0 760 Q 800 650 1600 760 L 1600 900 L 0 900 Z"/><g class="w-rows">{rows}</g></g>
{flies}"##,
        milky = milky,
        stars = stars(6000, 110, 560.0),
        small_torii = torii(420.0, 640.0, 90.0, 96.0),
        cat = sitting_cat(820.0, 648.0, 0.75, false),
        rows = rows,
        flies = flies,
    );
    svg("w-lavender", &defs, &body)
}

// ---------- Cyber · Matrix: code rain alley ----------

const GLYPHS: &[&str] =
    &["ア", "カ", "サ", "タ", "ナ", "ハ", "マ", "ヤ", "ラ", "ワ", "ネ", "コ", "0", "1", "ﾈ", "ｺ", "猫", "ﾂ", "ﾐ", "ﾘ"];

pub fn matrix() -> String {
    let mut rain = String::from(r##"<g class="w-rain">"##);
    for c in 0..42u32 {
        let x = 10.0 + c as f64 * 38.5 + rnd(c + 7000) * 10.0;
        let dur = 6.0 + rnd(c + 7100) * 8.0;
        let d = rnd(c + 7200) * dur;
        let _ = write!(
            rain,
            r##"<g class="w-col" style="animation-duration:{dur:.1}s;animation-delay:-{d:.1}s"><text x="{x:.0}" y="0">"##
        );
        let n = 14 + (rnd(c + 7300) * 12.0) as u32;
        for k in 0..n {
            let g = GLYPHS[((rnd(c * 100 + k + 7400) * GLYPHS.len() as f64) as usize).min(GLYPHS.len() - 1)];
            let cls = if k == n - 1 { r##" class="head""## } else { "" };
            let _ = write!(
                rain,
                r##"<tspan x="{x:.0}" dy="26"{cls} style="opacity:{o:.2}">{g}</tspan>"##,
                o = (k as f64 + 1.0) / n as f64
            );
        }
        rain.push_str("</text></g>");
    }
    rain.push_str("</g>");
    let building = |x: f64, w: f64, top: f64, flip: bool| {
        let mut s = format!(
            r##"<rect class="w-wallc" x="{x:.0}" y="{top:.0}" width="{w:.0}" height="{h:.0}"/>"##,
            h = 900.0 - top
        );
        let mut y = top + 60.0;
        while y < 820.0 {
            let _ = write!(
                s,
                r##"<path class="w-escape" d="M{a:.0} {y:.0} L {b:.0} {y:.0} M{a:.0} {y2:.0} L {b:.0} {y2:.0} M{a:.0} {y:.0} L {a:.0} {y2:.0}"/>"##,
                a = if flip { x } else { x + w - 90.0 },
                b = if flip { x + 90.0 } else { x + w },
                y2 = y + 30.0
            );
            y += 140.0;
        }
        s
    };
    let body = format!(
        r##"<rect width="1600" height="900" class="w-matrix-bg"/>
{rain}
<g class="w-layer mid">
  {lb}{rb}
  <g class="w-sign s1"><rect x="330" y="300" width="70" height="180" rx="6"/><text x="365" y="350">猫</text><text x="365" y="400">カ</text><text x="365" y="450">フ</text></g>
  <g class="w-sign s2"><rect x="1180" y="380" width="150" height="56" rx="8"/><text x="1255" y="420">NEKO</text></g>
</g>
<rect class="w-street" y="780" width="1600" height="120"/>
<g class="w-puddles"><ellipse cx="700" cy="830" rx="180" ry="12"/><ellipse cx="1100" cy="860" rx="120" ry="9"/></g>
<g class="w-layer near">
  {cat}
  <g class="w-eyes"><circle cx="784" cy="712" r="3.5"/><circle cx="800" cy="712" r="3.5"/></g>
</g>"##,
        rain = rain,
        lb = building(0.0, 360.0, 160.0, true),
        rb = building(1240.0, 360.0, 220.0, false),
        cat = sitting_cat(792.0, 780.0, 0.95, false).replace("w-cat\"", "w-cat w-cat-front\""),
    );
    svg("w-matrix", "", &body)
}

// ---------- Cyber · Synthwave: retro sunset drive ----------

pub fn synthwave() -> String {
    let defs = format!(
        r##"{}<linearGradient id="sw-sun" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#ffe86b"/><stop offset=".5" stop-color="#ff8a4c"/><stop offset="1" stop-color="#ff3c8e"/></linearGradient>
        <mask id="sw-stripes"><rect x="500" y="100" width="600" height="500" fill="#fff"/>{stripes}</mask>
        <clipPath id="sw-floor"><rect y="560" width="1600" height="340"/></clipPath>"##,
        sky_gradient("sw-sky", "--w-sky1", "--w-sky2", "--w-sky3"),
        stripes = (0..7)
            .map(|i| format!(
                r##"<rect x="500" y="{y}" width="600" height="{h}" fill="#000"/>"##,
                y = 400 + i * 22,
                h = 3 + i * 2
            ))
            .collect::<String>()
    );
    let mut grid = String::from(r##"<g clip-path="url(#sw-floor)"><g class="w-grid-h">"##);
    for i in 0..14 {
        let t = i as f64 / 13.0;
        let y = 560.0 + t * t * 360.0;
        let _ = write!(grid, r##"<path d="M0 {y:.0} L 1600 {y:.0}"/>"##);
    }
    grid.push_str(r##"</g><g class="w-grid-v">"##);
    for i in -12..=12 {
        let x = 800.0 + i as f64 * 140.0;
        let _ = write!(grid, r##"<path d="M800 560 L {x:.0} 900"/>"##);
    }
    grid.push_str("</g></g>");
    let palm = |x: f64, s: f64, flip: f64| {
        format!(
            r##"<g class="w-palm" transform="translate({x:.0} 900) scale({sx:.2} {s:.2})">
  <path class="w-palm-trunk" d="M0 0 C 10 -120 30 -240 60 -330"/>
  <path class="w-frond" d="M60 -330 C 0 -360 -60 -340 -110 -290 C -50 -320 10 -320 60 -330 Z"/>
  <path class="w-frond" d="M60 -330 C 120 -370 190 -350 230 -300 C 170 -330 110 -330 60 -330 Z"/>
  <path class="w-frond" d="M60 -330 C 30 -400 -20 -430 -80 -420 C -20 -400 20 -370 60 -330 Z"/>
  <path class="w-frond" d="M60 -330 C 100 -410 160 -430 210 -410 C 150 -400 100 -370 60 -330 Z"/>
  <path class="w-frond" d="M60 -330 C 40 -300 20 -250 30 -200 C 40 -250 50 -290 60 -330 Z"/>
</g>"##,
            sx = s * flip
        )
    };
    let body = format!(
        r##"<rect width="1600" height="900" fill="url(#sw-sky)"/>
{stars}
<g class="w-layer far"><circle cx="800" cy="420" r="180" fill="url(#sw-sun)" mask="url(#sw-stripes)" class="w-synth-sun"/></g>
<g class="w-layer far"><path class="w-mount" d="M0 560 L 120 470 L 220 520 L 360 420 L 470 500 L 560 460 L 660 560 Z M940 560 L 1040 470 L 1150 520 L 1260 410 L 1380 500 L 1480 450 L 1600 520 L 1600 560 Z"/>
<path class="w-mount-line" d="M0 560 L 120 470 L 220 520 L 360 420 L 470 500 L 560 460 L 660 560 M940 560 L 1040 470 L 1150 520 L 1260 410 L 1380 500 L 1480 450 L 1600 520"/></g>
<rect class="w-floor" y="560" width="1600" height="340"/>
{grid}
<rect class="w-horizon" y="558" width="1600" height="3"/>
<g class="w-layer near">{p1}{p2}{p3}</g>
<g class="w-layer near"><path class="w-rock" d="M330 900 Q 360 790 460 770 Q 560 760 600 900 Z"/>{cat}</g>"##,
        stars = stars(8000, 60, 380.0),
        grid = grid,
        p1 = palm(120.0, 1.1, 1.0),
        p2 = palm(1500.0, 1.2, -1.0),
        p3 = palm(1330.0, 0.8, -1.0),
        cat = sitting_cat(470.0, 776.0, 0.85, false),
    );
    svg("w-synth", &defs, &body)
}

// ---------- Cyber · Ice: aurora over a frozen city ----------

pub fn ice() -> String {
    let defs = format!(
        r##"{}<linearGradient id="ic-aur" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#7cf5d6" stop-opacity="0"/><stop offset=".55" stop-color="#7cf5d6" stop-opacity=".55"/><stop offset="1" stop-color="#8b7cff" stop-opacity="0"/></linearGradient>
        <linearGradient id="ic-aur2" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#c49bff" stop-opacity="0"/><stop offset=".5" stop-color="#c49bff" stop-opacity=".45"/><stop offset="1" stop-color="#5ee0ff" stop-opacity="0"/></linearGradient>
        <linearGradient id="ic-ice" x1="0" y1="0" x2="0" y2="1"><stop offset="0" style="stop-color:var(--w-ice1)"/><stop offset="1" style="stop-color:var(--w-ice2)"/></linearGradient>"##,
        sky_gradient("ic-sky", "--w-sky1", "--w-sky2", "--w-sky3")
    );
    let mut towers = String::new();
    for i in 0..12u32 {
        let x = 820.0 + i as f64 * 64.0 + rnd(i + 9000) * 20.0;
        let h = 120.0 + rnd(i + 9100) * 260.0;
        let w = 30.0 + rnd(i + 9200) * 30.0;
        let top = 600.0 - h;
        let _ = write!(
            towers,
            r##"<path class="w-tower" d="M{x:.0} 600 L {x:.0} {t2:.0} L {xm:.0} {top:.0} L {xr:.0} {t2:.0} L {xr:.0} 600 Z"/><path class="w-tower-line" d="M{xm:.0} {t3:.0} L {xm:.0} 590"/>"##,
            t2 = top + 30.0,
            xm = x + w / 2.0,
            xr = x + w,
            t3 = top + 40.0
        );
    }
    let body = format!(
        r##"<rect width="1600" height="900" fill="url(#ic-sky)"/>
{stars}
<g class="w-aurora a1"><path d="M-200 300 C 200 120 500 380 900 200 C 1200 70 1500 260 1800 140 L 1800 420 C 1500 520 1200 330 900 470 C 500 640 200 380 -200 560 Z" fill="url(#ic-aur)"/></g>
<g class="w-aurora a2"><path d="M-200 220 C 300 80 600 300 1000 160 C 1300 60 1600 200 1800 100 L 1800 300 C 1600 380 1300 240 1000 340 C 600 480 300 260 -200 420 Z" fill="url(#ic-aur2)"/></g>
<g class="w-layer far"><path class="w-glacier" d="M0 600 L 140 470 L 230 520 L 380 400 L 520 520 L 640 460 L 760 600 Z"/><path class="w-glacier-hi" d="M140 470 L 170 500 L 230 520 Z M380 400 L 420 450 L 470 470 Z"/></g>
<g class="w-layer mid">{towers}</g>
<rect y="600" width="1600" height="300" fill="url(#ic-ice)"/>
<g class="w-reflect" transform="translate(0 1200) scale(1 -1)">{towers}</g>
<g class="w-cracks"><path d="M200 700 L 340 730 L 420 710 M900 760 L 1040 800 L 1120 780 L 1230 820 M500 840 L 640 860"/></g>
<g class="walker w-ice-walker"><g class="walker-flip"><g class="walk-cat">
  <path class="wc-tail" d="M-18 -18 C -32 -22 -34 -38 -26 -44"/>
  <ellipse class="wc-body" cx="0" cy="-16" rx="20" ry="10"/>
  <circle class="wc-head" cx="20" cy="-28" r="10"/>
  <path class="wc-ear" d="M13 -35 L 14 -46 L 20 -38 Z M23 -37 L 28 -46 L 29 -34 Z"/>
  <rect class="wc-visor" x="18" y="-31" width="12" height="4" rx="2"/>
  <path class="leg l1" d="M-12 -8 L -14 0"/><path class="leg l2" d="M-6 -8 L -4 0"/>
  <path class="leg l1" d="M8 -8 L 10 0"/><path class="leg l2" d="M14 -8 L 12 0"/>
</g></g></g>"##,
        stars = stars(9500, 90, 420.0),
        towers = towers,
    );
    svg("w-ice", &defs, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worlds_are_well_formed_svg() {
        for (name, s) in [
            ("sakura", sakura()),
            ("matcha", matcha()),
            ("mochi", mochi()),
            ("lavender", lavender()),
            ("matrix", matrix()),
            ("synthwave", synthwave()),
            ("ice", ice()),
        ] {
            assert!(s.starts_with("<svg") && s.ends_with("</svg>"), "{name}");
            for tag in ["<g", "<text", "<defs", "<mask", "<clipPath"] {
                let open = s.matches(tag).count();
                let close = s.matches(&format!("</{}", &tag[1..])).count();
                assert_eq!(open, close, "{name}: unbalanced {tag} ({open} vs {close})");
            }
            assert!(!s.contains("NaN") && !s.contains("inf"), "{name} has bad numbers");
            assert!(!s.contains(" --") && !s.contains("\"--"), "{name} has a doubled minus sign");
        }
    }
}
