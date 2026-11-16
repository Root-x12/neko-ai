//! Blossom valley: cherry trees framing a mountain lake with a pagoda on a mossy island.
//! The default scene for both moods (Neko: spring day / moonlit night, Cyber: neon night).
//!
//! The scene is a stack of separate SVGs rather than one. The detailed layers (mountains,
//! rocks, thousands of blossom circles) never animate inside, so the browser paints each once
//! and then only moves it (parallax, the canopies swaying). What moves inside a layer (water,
//! glints, birds, the cats) uses transform and opacity only, which the browser animates without
//! repainting, and there are no blur or glow filters: soft edges are gradients. Layers are pinned
//! left, centre or right, so on a narrow phone screen the trees and the pagoda slide in
//! towards the mountains instead of being cropped away.

use std::fmt::Write;

use super::worlds::{birds, lantern, rnd, sitting_cat};

/// Main mountain outline (before the group's shift to the right).
const MASSIF: &str = "M300 660 L 380 560 L 430 500 L 470 450 L 520 400 L 550 340 L 590 290 L 615 240 L 650 185 L 672 210 L 700 235 L 730 205 L 765 160 L 792 132 L 818 170 L 845 205 L 868 185 L 892 168 L 930 230 L 965 275 L 1000 330 L 1040 380 L 1090 440 L 1150 520 L 1210 590 L 1270 660 Z";
/// The forested slope that falls into the valley from the right.
const SLOPE: &str = "M960 660 L 1040 560 L 1110 470 L 1170 400 L 1230 330 L 1290 270 L 1340 300 L 1400 230 L 1460 170 L 1530 120 L 1610 60 L 1800 -20 L 1800 660 Z";

fn layer(class: &str, align: &str, cyber: bool, defs: &str, body: &str) -> String {
    let mode = if cyber { " cyber" } else { "" };
    format!(
        r##"<svg class="scene w-valley{mode} {class}" viewBox="0 0 1600 900" preserveAspectRatio="{align} slice"><defs>{defs}</defs>{body}</svg>"##
    )
}

fn grad3(id: &str, a: &str, b: &str, c: &str) -> String {
    format!(
        r##"<linearGradient id="{id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" style="stop-color:var({a})"/><stop offset=".55" style="stop-color:var({b})"/><stop offset="1" style="stop-color:var({c})"/></linearGradient>"##
    )
}

fn grad2(id: &str, a: &str, b: &str) -> String {
    format!(
        r##"<linearGradient id="{id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" style="stop-color:var({a})"/><stop offset="1" style="stop-color:var({b})"/></linearGradient>"##
    )
}

/// A radial fade from a colour variable to nothing: the cheap stand-in for a blur.
fn soft(id: &str, var: &str, strength: f64) -> String {
    format!(
        r##"<radialGradient id="{id}"><stop offset="0" style="stop-color:var({var})" stop-opacity="{strength}"/><stop offset=".6" style="stop-color:var({var})" stop-opacity="{:.2}"/><stop offset="1" style="stop-color:var({var})" stop-opacity="0"/></radialGradient>"##,
        strength * 0.45
    )
}

/// A mass of blossom (or leaves): shadowed underside, mid tone, sunlit tops, loose specks
/// around the rim. `unit` is the size of one puff.
fn bloom(seed: u32, cx: f64, cy: f64, rx: f64, ry: f64, n: u32, unit: f64) -> String {
    let pts: Vec<(f64, f64, f64)> = (0..n)
        .map(|i| {
            let a = rnd(seed + i) * std::f64::consts::TAU;
            let d = rnd(seed + i + 1000).sqrt();
            let r = unit * (0.7 + rnd(seed + i + 2000) * 0.9);
            (cx + a.cos() * d * rx, cy + a.sin() * d * ry, r)
        })
        .collect();
    let mut s = String::from("<g>");
    for &(x, y, r) in &pts {
        let _ = write!(s, r##"<circle class="b0" cx="{x:.0}" cy="{:.0}" r="{:.1}"/>"##, y + r * 0.35, r * 1.05);
    }
    for (i, &(x, y, r)) in pts.iter().enumerate() {
        if i % 4 != 0 {
            let _ = write!(s, r##"<circle class="b1" cx="{x:.0}" cy="{y:.0}" r="{:.1}"/>"##, r * 0.9);
        }
    }
    for (i, &(x, y, r)) in pts.iter().enumerate() {
        if i % 2 == 0 {
            let _ = write!(
                s,
                r##"<circle class="b2" cx="{:.0}" cy="{:.0}" r="{:.1}"/>"##,
                x - r * 0.2,
                y - r * 0.35,
                r * 0.62
            );
        }
    }
    for (i, &(x, y, r)) in pts.iter().enumerate() {
        if i % 4 == 0 {
            let _ = write!(
                s,
                r##"<circle class="b3" cx="{:.0}" cy="{:.0}" r="{:.1}"/>"##,
                x - r * 0.3,
                y - r * 0.5,
                r * 0.24
            );
        }
    }
    for i in 0..n / 2 {
        let a = rnd(seed + i + 3000) * std::f64::consts::TAU;
        let d = 1.0 + rnd(seed + i + 4000) * 0.24;
        let r = unit * (0.14 + rnd(seed + i + 5000) * 0.26);
        let cls = if i % 2 == 0 { "b1" } else { "b2" };
        let _ = write!(
            s,
            r##"<circle class="{cls}" cx="{:.0}" cy="{:.0}" r="{r:.1}"/>"##,
            cx + a.cos() * d * rx,
            cy + a.sin() * d * ry
        );
    }
    s.push_str("</g>");
    s
}

/// A five-petal flower seen up close.
fn flower(x: f64, y: f64, r: f64, turn: f64) -> String {
    let mut s = format!(r##"<g transform="translate({x:.0} {y:.0}) rotate({turn:.0})">"##);
    for k in 0..5 {
        let a = f64::from(k) * std::f64::consts::TAU / 5.0;
        let _ = write!(
            s,
            r##"<ellipse class="b2" cx="{:.1}" cy="{:.1}" rx="{:.1}" ry="{:.1}" transform="rotate({:.0} {:.1} {:.1})"/>"##,
            a.cos() * r * 0.62,
            a.sin() * r * 0.62,
            r * 0.62,
            r * 0.44,
            a.to_degrees(),
            a.cos() * r * 0.62,
            a.sin() * r * 0.62
        );
    }
    let _ = write!(s, r##"<circle class="b0" r="{:.1}"/><circle class="b3" r="{:.1}"/></g>"##, r * 0.3, r * 0.14);
    s
}

fn pine(x: f64, y: f64, h: f64, alt: bool) -> String {
    let w = h * 0.44;
    let mut d = String::new();
    for k in 0..3 {
        let k = f64::from(k);
        let tip = y - h + k * h * 0.24;
        let base = tip + h * 0.5;
        let half = w * (0.5 + k * 0.25);
        let _ = write!(d, "M{x:.0} {tip:.0} L {:.0} {base:.0} L {:.0} {base:.0} Z ", x - half, x + half);
    }
    let cls = if alt { "va-pine alt" } else { "va-pine" };
    format!(r##"<path class="{cls}" d="{}"/>"##, d.trim_end())
}

fn cumulus(seed: u32, x: f64, y: f64, w: f64) -> String {
    let mut s = String::from("<g>");
    let n = 8;
    let mut lobes = Vec::new();
    for i in 0..n {
        let t = f64::from(i) / f64::from(n - 1);
        let arch = 1.0 - (t - 0.5).abs() * 2.0;
        let r = w * (0.1 + 0.13 * arch) * (0.8 + rnd(seed + i) * 0.5);
        lobes.push((x + (t - 0.5) * w, y - r * 0.5 - arch * w * 0.12 * rnd(seed + i + 50), r));
    }
    for &(cx, cy, r) in &lobes {
        let _ = write!(s, r##"<circle class="c0" cx="{cx:.0}" cy="{:.0}" r="{r:.0}"/>"##, cy + r * 0.25);
    }
    let _ = write!(s, r##"<ellipse class="c0" cx="{x:.0}" cy="{y:.0}" rx="{:.0}" ry="{:.0}"/>"##, w * 0.56, w * 0.07);
    for &(cx, cy, r) in &lobes {
        let _ = write!(s, r##"<circle class="c1" cx="{cx:.0}" cy="{cy:.0}" r="{:.0}"/>"##, r * 0.92);
    }
    for &(cx, cy, r) in &lobes {
        let _ = write!(
            s,
            r##"<circle class="c2" cx="{:.0}" cy="{:.0}" r="{:.0}"/>"##,
            cx + r * 0.2,
            cy - r * 0.28,
            r * 0.6
        );
    }
    s.push_str("</g>");
    s
}

fn water_rock(x: f64, y: f64, w: f64) -> String {
    format!(
        r##"<ellipse class="va-rock-shade" cx="{x:.0}" cy="{:.0}" rx="{:.0}" ry="{:.0}"/>
<path class="va-rock" d="M{:.0} {y:.0} Q {:.0} {:.0} {:.0} {:.0} Q {:.0} {:.0} {:.0} {y:.0} Z"/>
<path class="va-moss" d="M{:.0} {:.0} Q {:.0} {:.0} {:.0} {:.0} Q {:.0} {:.0} {:.0} {:.0} Q {x:.0} {:.0} {:.0} {:.0} Z"/>"##,
        y + 3.0,
        w * 0.66,
        w * 0.09,
        x - w * 0.5,
        x - w * 0.45,
        y - w * 0.42,
        x - w * 0.1,
        y - w * 0.5,
        x + w * 0.35,
        y - w * 0.52,
        x + w * 0.5,
        x - w * 0.42,
        y - w * 0.28,
        x - w * 0.3,
        y - w * 0.46,
        x - w * 0.1,
        y - w * 0.5,
        x + w * 0.3,
        y - w * 0.52,
        x + w * 0.44,
        y - w * 0.2,
        y - w * 0.36,
        x - w * 0.42,
        y - w * 0.28
    )
}

/// One building of the Cyber skyline on the far shore: left edge, width, height.
fn tower(i: u32) -> (f64, f64, f64) {
    (
        350.0 + f64::from(i) * 23.0 + rnd(i + 40_000) * 8.0,
        13.0 + rnd(i + 41_000) * 12.0,
        26.0 + rnd(i + 42_000).powi(2) * 110.0,
    )
}

// ---------- layers, far to near ----------

fn sky(cyber: bool) -> String {
    let defs = format!(
        r##"{}<radialGradient id="va-sun"><stop offset="0" style="stop-color:var(--va-sun)" stop-opacity=".95"/><stop offset=".35" style="stop-color:var(--va-sun)" stop-opacity=".45"/><stop offset="1" style="stop-color:var(--va-sun)" stop-opacity="0"/></radialGradient>
<linearGradient id="va-holo" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#22e4ff"/><stop offset="1" stop-color="#ff3cac"/></linearGradient>"##,
        grad3("va-sky", "--va-sky1", "--va-sky2", "--va-sky3")
    );
    let mut stars = String::from(r##"<g class="va-stars">"##);
    for i in 0..170u32 {
        let _ = write!(
            stars,
            r##"<circle cx="{:.0}" cy="{:.0}" r="{:.1}" opacity="{:.2}"/>"##,
            -100.0 + rnd(i + 11) * 1800.0,
            -600.0 + rnd(i + 511) * 1120.0,
            0.5 + rnd(i + 911) * 1.4,
            0.4 + rnd(i + 1311) * 0.6
        );
    }
    stars.push_str("</g>");
    let holo = if cyber {
        r##"<g class="va-holo"><circle cx="1060" cy="150" r="96" fill="url(#va-holo)" opacity=".28"/>
<circle cx="1060" cy="150" r="96" fill="none" stroke="url(#va-holo)" stroke-width="2.5"/>
<ellipse cx="1060" cy="150" rx="150" ry="26" fill="none" stroke="#22e4ff" stroke-width="1.6" opacity=".7" transform="rotate(-14 1060 150)"/>
<path d="M968 150 L 1152 150 M980 118 L 1140 118 M980 182 L 1140 182 M1060 54 L 1060 246" stroke="#22e4ff" stroke-width="1" opacity=".35"/></g>"##
    } else {
        ""
    };
    let body = format!(
        r##"<rect x="-200" y="-1200" width="2000" height="2300" fill="url(#va-sky)"/>
{stars}
<circle cx="1060" cy="130" r="330" fill="url(#va-sun)"/>
<circle class="va-moon" cx="1060" cy="150" r="42"/>
{holo}
<g class="va-wisps"><path d="M1010 70 L 1250 118"/><path d="M1150 96 L 1330 150"/><path d="M520 150 L 700 130"/><path d="M240 250 L 420 232"/></g>"##
    );
    layer("va-sky", "xMidYMax", cyber, &defs, &body)
}

fn clouds(cyber: bool) -> String {
    let body = format!(
        r##"{}{}{}{}<g class="va-thin"><ellipse cx="620" cy="120" rx="210" ry="14"/><ellipse cx="1330" cy="330" rx="260" ry="18"/><ellipse cx="180" cy="210" rx="230" ry="16"/></g>"##,
        cumulus(100, 1210.0, 300.0, 460.0),
        cumulus(200, 930.0, 232.0, 280.0),
        cumulus(300, 300.0, 330.0, 380.0),
        cumulus(400, 1560.0, 220.0, 330.0),
    );
    layer("va-clouds", "xMidYMax", cyber, "", &body)
}

fn mountains(cyber: bool) -> String {
    let defs = format!(
        r##"{}{}{}{}{}<clipPath id="va-massif-clip"><path d="{MASSIF}"/></clipPath><clipPath id="va-slope-clip"><path d="{SLOPE}"/></clipPath>
<linearGradient id="va-haze" x1="0" y1="0" x2="0" y2="1"><stop offset="0" style="stop-color:var(--va-haze)" stop-opacity="0"/><stop offset="1" style="stop-color:var(--va-haze)" stop-opacity=".75"/></linearGradient>"##,
        grad2("va-mt", "--va-mt1", "--va-mt2"),
        grad2("va-sl", "--va-sl1", "--va-sl2"),
        grad2("va-hill", "--va-hill1", "--va-hill2"),
        soft("va-mistg", "--va-mist", 1.0),
        lake_defs(),
    );
    // Rock strata: short slanted strokes, dark on the shaded side and pale on the lit one.
    let mut strata = String::new();
    for i in 0..150u32 {
        let x = 330.0 + rnd(i + 20_000) * 920.0;
        let y = 160.0 + rnd(i + 21_000) * 480.0;
        let len = 18.0 + rnd(i + 22_000) * 60.0;
        let lean = if rnd(i + 23_000) > 0.5 { 0.42 } else { -0.3 };
        let cls = if i % 3 == 0 { "va-strata hi" } else { "va-strata" };
        let _ = write!(strata, r##"<path class="{cls}" d="M{x:.0} {y:.0} l {:.0} {len:.0}"/>"##, len * lean);
    }
    let mut pines = String::new();
    for i in 0..230u32 {
        let x = 990.0 + rnd(i + 30_000) * 760.0;
        let top = (660.0 - (x - 960.0) * 0.92).max(60.0);
        let y = top + 30.0 + rnd(i + 31_000).powf(0.7) * (650.0 - top - 30.0);
        let h = 20.0 + (y - 200.0).max(0.0) / 460.0 * 30.0 + rnd(i + 32_000) * 10.0;
        pines.push_str(&pine(x, y, h, i % 3 == 0));
    }
    let mut left_pines = String::new();
    for i in 0..34u32 {
        let x = -80.0 + rnd(i + 33_000) * 640.0;
        let top = 500.0 + (x.max(0.0) / 660.0) * 110.0;
        let y = top + 20.0 + rnd(i + 34_000) * (640.0 - top);
        left_pines.push_str(&pine(x, y.min(648.0), 22.0 + rnd(i + 35_000) * 20.0, i % 2 == 0));
    }
    // The far shore: a line of blossom along the water, a small temple, and in Cyber a skyline.
    let mut shore = String::new();
    if cyber {
        shore.push_str(r##"<g class="va-city">"##);
        for i in 0..30u32 {
            let (x, w, h) = tower(i);
            let _ = write!(
                shore,
                r##"<rect class="va-bld" x="{x:.0}" y="{:.0}" width="{w:.0}" height="{:.0}"/>"##,
                642.0 - h,
                h
            );
            let rows = (h / 9.0) as u32;
            for r in 0..rows {
                if rnd(i * 31 + r + 43_000) > 0.45 {
                    let cls = if rnd(i * 17 + r + 44_000) > 0.6 { "va-bld-win alt" } else { "va-bld-win" };
                    let _ = write!(
                        shore,
                        r##"<rect class="{cls}" x="{:.0}" y="{:.0}" width="{:.0}" height="2.4"/>"##,
                        x + 2.5,
                        646.0 - h + f64::from(r) * 9.0,
                        w - 5.0
                    );
                }
            }
        }
        shore.push_str("</g>");
    }
    shore.push_str(r##"<g class="va-far-bloom">"##);
    for i in 0..44u32 {
        let x = 300.0 + f64::from(i) * 17.0 + rnd(i + 45_000) * 10.0;
        let y = 634.0 - rnd(i + 46_000) * 12.0;
        shore.push_str(&bloom(50_000 + i * 97, x, y, 15.0, 8.0, 6, 6.5));
    }
    shore.push_str("</g>");
    let ridge_shift = r##"transform="translate(90 0)""##;
    let body = format!(
        r##"<path class="va-mt-far" d="M-200 660 L -200 430 L -40 350 L 60 320 L 140 372 L 232 296 L 330 388 L 420 468 L 520 560 L 620 660 Z"/>
<path class="va-snow soft" d="M60 320 L 140 372 L 118 384 L 84 356 L 40 372 Z M232 296 L 330 388 L 290 384 L 258 348 L 214 356 Z"/>
<g {ridge_shift}>
  <path d="{MASSIF}" fill="url(#va-mt)"/>
  <g clip-path="url(#va-massif-clip)">
    <path class="va-mt-sh" d="M650 185 L 615 240 L 590 290 L 550 340 L 520 400 L 470 520 L 560 480 L 600 360 L 630 300 L 640 240 Z"/>
    <path class="va-mt-sh" d="M792 132 L 765 160 L 730 205 L 740 300 L 720 400 L 700 520 L 760 480 L 780 340 L 775 240 Z"/>
    <path class="va-mt-sh" d="M892 168 L 868 185 L 860 280 L 880 380 L 900 520 L 950 480 L 925 360 L 905 260 Z"/>
    <path class="va-mt-sh deep" d="M672 210 L 700 235 L 690 330 L 650 450 L 640 560 L 610 660 L 540 660 L 590 520 L 636 400 Z"/>
    <path class="va-mt-sh deep" d="M845 205 L 868 185 L 862 290 L 850 400 L 870 540 L 860 660 L 800 660 L 822 520 L 830 380 Z"/>
    <path class="va-mt-sh" d="M1000 330 L 1040 380 L 1090 440 L 1060 560 L 1090 660 L 1010 660 L 990 520 L 1010 420 Z"/>
    {strata}
    <path class="va-snow" d="M650 185 L 672 210 L 668 260 L 690 330 L 662 300 L 656 360 L 644 280 L 648 240 Z"/>
    <path class="va-snow" d="M792 132 L 818 170 L 845 205 L 850 262 L 832 240 L 836 322 L 812 270 L 808 340 L 796 250 L 800 200 Z"/>
    <path class="va-snow" d="M892 168 L 930 230 L 965 275 L 976 332 L 946 300 L 952 372 L 922 312 L 918 380 L 904 300 L 905 232 Z"/>
    <path class="va-snow" d="M1000 330 L 1040 380 L 1064 432 L 1030 404 L 1034 452 L 1010 400 Z M700 235 L 730 205 L 736 262 L 716 302 L 712 262 Z"/>
    <path class="va-snow soft" d="M560 420 L 600 470 L 580 474 L 590 520 L 556 470 Z M760 420 L 800 480 L 776 478 L 784 540 L 752 480 Z M960 430 L 1000 500 L 974 496 L 980 556 L 950 492 Z"/>
    <rect x="200" y="420" width="1200" height="240" fill="url(#va-haze)"/>
  </g>
  <path class="va-ridge glow" d="M430 500 L 470 450 L 520 400 L 550 340 L 590 290 L 615 240 L 650 185 L 672 210 L 700 235 L 730 205 L 765 160 L 792 132 L 818 170 L 845 205 L 868 185 L 892 168 L 930 230 L 965 275 L 1000 330 L 1040 380 L 1090 440 L 1150 520"/>
  <path class="va-ridge" d="M430 500 L 470 450 L 520 400 L 550 340 L 590 290 L 615 240 L 650 185 L 672 210 L 700 235 L 730 205 L 765 160 L 792 132 L 818 170 L 845 205 L 868 185 L 892 168 L 930 230 L 965 275 L 1000 330 L 1040 380 L 1090 440 L 1150 520"/>
</g>
<path class="va-fall-bed" d="M944 330 C 938 400 950 480 946 560 C 944 590 952 600 950 618"/>
<g class="va-mist"><ellipse cx="880" cy="592" rx="460" ry="70"/><ellipse cx="620" cy="560" rx="300" ry="54"/><ellipse cx="1130" cy="520" rx="280" ry="50"/><ellipse cx="948" cy="616" rx="90" ry="34"/></g>
<path d="{SLOPE}" fill="url(#va-sl)"/>
<g clip-path="url(#va-slope-clip)">
  <path class="va-snow soft" d="M1290 270 L 1340 300 L 1320 330 L 1290 310 L 1262 330 Z M1460 170 L 1530 120 L 1610 60 L 1640 110 L 1560 150 L 1500 200 Z"/>
  {pines}
  <rect x="940" y="470" width="900" height="190" fill="url(#va-haze)" opacity=".7"/>
</g>
<path class="va-ridge glow" d="M1040 560 L 1110 470 L 1170 400 L 1230 330 L 1290 270 L 1340 300 L 1400 230 L 1460 170 L 1530 120 L 1610 60"/>
<path class="va-ridge" d="M1040 560 L 1110 470 L 1170 400 L 1230 330 L 1290 270 L 1340 300 L 1400 230 L 1460 170 L 1530 120 L 1610 60"/>
<path d="M-200 670 L -200 500 Q 80 448 270 520 Q 430 580 570 612 Q 680 634 780 670 Z" fill="url(#va-hill)"/>
<path class="va-hill-hi" d="M-200 520 Q 60 470 250 530 Q 380 574 470 600 Q 300 566 120 560 Q -40 556 -200 580 Z"/>
{left_pines}
<g class="va-mist low"><ellipse cx="760" cy="640" rx="560" ry="44"/><ellipse cx="240" cy="610" rx="360" ry="40"/></g>
{shore}
<g class="va-temple"><rect x="604" y="622" width="34" height="18"/><path d="M592 624 Q 621 606 650 624 Z"/><path d="M600 612 Q 621 598 642 612 Z"/></g>
{lake}"##,
        lake = lake(cyber)
    );
    layer("va-land w-layer far", "xMidYMax", cyber, &defs, &body)
}

fn lake_defs() -> String {
    format!(
        r##"{}{}{}<linearGradient id="va-lake-fade" x1="0" y1="0" x2="0" y2="1"><stop offset="0" style="stop-color:var(--va-lake2)" stop-opacity="0"/><stop offset=".7" style="stop-color:var(--va-lake3)" stop-opacity=".85"/></linearGradient>
<linearGradient id="va-neon-a" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#22e4ff" stop-opacity=".7"/><stop offset="1" stop-color="#22e4ff" stop-opacity="0"/></linearGradient>
<linearGradient id="va-neon-b" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#ff3cac" stop-opacity=".7"/><stop offset="1" stop-color="#ff3cac" stop-opacity="0"/></linearGradient>"##,
        grad3("va-lake", "--va-lake1", "--va-lake2", "--va-lake3"),
        soft("va-glitter", "--va-glint", 0.5),
        soft("va-bloomg", "--va-b1", 0.6),
    )
}

/// The water itself, with what it mirrors. Drawn into the same layer as the mountains.
fn lake(cyber: bool) -> String {
    let mut streaks = String::new();
    for i in 0..46u32 {
        let y = 650.0 + rnd(i + 60_000).powf(1.4) * 250.0;
        let w = 40.0 + (y - 640.0) * 0.9 * rnd(i + 61_000);
        let _ = write!(
            streaks,
            r##"<rect x="{:.0}" y="{y:.0}" width="{w:.0}" height="{:.1}" rx="1"/>"##,
            -150.0 + rnd(i + 62_000) * 1800.0,
            1.0 + (y - 640.0) / 180.0
        );
    }
    let neon = if cyber {
        let mut s = String::from(r##"<g class="va-neon-refl">"##);
        for i in 0..30u32 {
            let (x, w, h) = tower(i);
            let fill = if i % 3 == 0 { "va-neon-b" } else { "va-neon-a" };
            let _ = write!(
                s,
                r##"<rect x="{:.0}" y="644" width="{:.0}" height="{:.0}" fill="url(#{fill})"/>"##,
                x + 2.0,
                w - 4.0,
                h * 1.5
            );
        }
        s.push_str("</g>");
        s
    } else {
        String::new()
    };
    format!(
        r##"<rect x="-200" y="640" width="2000" height="400" fill="url(#va-lake)"/>
<g class="va-reflect" transform="translate(0 1280) scale(1 -1)">
  <path d="{MASSIF}" transform="translate(90 0)" class="va-refl-mt"/>
  <path d="{SLOPE}" class="va-refl-sl"/>
  <path class="va-refl-hill" d="M-200 670 L -200 500 Q 80 448 270 520 Q 430 580 570 612 Q 680 634 780 670 Z"/>
</g>
<g class="va-refl-bloom"><ellipse cx="690" cy="660" rx="520" ry="34"/><ellipse cx="1180" cy="700" rx="340" ry="44"/><ellipse cx="240" cy="700" rx="380" ry="50"/></g>
{neon}
<rect x="-200" y="640" width="2000" height="400" fill="url(#va-lake-fade)"/>
<ellipse cx="930" cy="800" rx="330" ry="150" fill="url(#va-glitter)"/>
<g class="va-streaks">{streaks}</g>
<rect class="va-shoreline" x="-200" y="639" width="2000" height="2.5"/>"##
    )
}

/// Everything in the middle of the picture that moves: the current, sun glints, petals on the
/// water, the waterfall, birds, and a boat (Neko) or drones (Cyber).
fn water_life(cyber: bool) -> String {
    // Stars and glints blink in a few groups rather than one by one: far fewer moving parts.
    let mut twinkle = String::from(r##"<g class="va-twinkle">"##);
    for g in 0..3u32 {
        let _ = write!(twinkle, r##"<g style="animation-delay:-{:.1}s">"##, f64::from(g) * 1.2);
        for i in 0..8u32 {
            let k = g * 8 + i;
            let _ = write!(
                twinkle,
                r##"<circle cx="{:.0}" cy="{:.0}" r="{:.1}"/>"##,
                rnd(k + 70_000) * 1600.0,
                -300.0 + rnd(k + 70_500) * 720.0,
                1.0 + rnd(k + 71_000) * 1.2
            );
        }
        twinkle.push_str("</g>");
    }
    twinkle.push_str("</g>");
    // The current: dashes laid out in a cell that repeats, so the loop has no seam.
    let flow = |seed: u32, y0: f64, y1: f64, per_cell: u32, cell: f64| {
        let mut s = String::new();
        for i in 0..per_cell {
            let y = y0 + rnd(seed + i) * (y1 - y0);
            let depth = (y - 640.0) / 260.0;
            let w = 14.0 + depth * 60.0 * (0.5 + rnd(seed + i + 300));
            let x = rnd(seed + i + 600) * cell;
            let o = 0.25 + rnd(seed + i + 900) * 0.5;
            let mut k = -1.0;
            while k * cell < 1900.0 {
                let _ = write!(
                    s,
                    r##"<rect x="{:.0}" y="{y:.0}" width="{w:.0}" height="{:.1}" rx="1.5" opacity="{o:.2}"/>"##,
                    x + k * cell,
                    1.2 + depth * 2.2
                );
                k += 1.0;
            }
        }
        s
    };
    let mut drift = String::new();
    for i in 0..18u32 {
        let y = 670.0 + rnd(i + 73_000) * 220.0;
        let s = 0.6 + (y - 640.0) / 260.0;
        let x = rnd(i + 73_500) * 600.0;
        let mut k = -1.0;
        while k * 600.0 < 1900.0 {
            let _ = write!(
                drift,
                r##"<ellipse cx="{:.0}" cy="{y:.0}" rx="{:.1}" ry="{:.1}" transform="rotate({:.0} {:.0} {y:.0})"/>"##,
                x + k * 600.0,
                5.0 * s,
                2.0 * s,
                rnd(i + 74_000) * 40.0 - 20.0,
                x + k * 600.0
            );
            k += 1.0;
        }
    }
    const GLINT_GROUPS: u32 = 7;
    let mut groups = vec![String::new(); GLINT_GROUPS as usize];
    for i in 0..98u32 {
        // Gathered into a path of light under the sun, thinning towards the edges.
        let spread = (rnd(i + 75_000) + rnd(i + 75_300) + rnd(i + 75_600)) / 3.0 - 0.5;
        let y = 680.0 + rnd(i + 76_000).powf(0.8) * 215.0;
        let x = 930.0 + spread * (360.0 + (y - 640.0) * 2.6);
        let s = 0.5 + (y - 640.0) / 200.0 + rnd(i + 77_000) * 0.5;
        let g = &mut groups[(i % GLINT_GROUPS) as usize];
        if i % 3 == 0 {
            let _ = write!(
                g,
                r##"<path transform="translate({x:.0} {y:.0}) scale({s:.2})" d="M0 -6 Q 0 0 9 0 Q 0 0 0 6 Q 0 0 -9 0 Q 0 0 0 -6 Z"/>"##
            );
        } else {
            let _ = write!(g, r##"<ellipse cx="{x:.0}" cy="{y:.0}" rx="{:.1}" ry="{:.1}"/>"##, 5.0 * s, 1.3 * s);
        }
    }
    let mut glints = String::new();
    for (k, g) in groups.iter().enumerate() {
        let _ = write!(
            glints,
            r##"<g style="animation-delay:-{:.2}s;animation-duration:{:.1}s">{g}</g>"##,
            k as f64 * 0.37,
            1.9 + (k % 3) as f64 * 0.6
        );
    }
    // The waterfall: drops sliding down behind a window, one pattern length per loop.
    let mut drops = String::new();
    for i in 0..22u32 {
        let y = 250.0 + f64::from(i) * 20.0;
        let _ = write!(
            drops,
            r##"<rect x="{:.1}" y="{y:.0}" width="{:.1}" height="{:.0}" rx="2"/>"##,
            939.0 + rnd(i % 4 + 79_000) * 7.0,
            2.5 + rnd(i % 4 + 79_100) * 3.0,
            9.0 + rnd(i % 4 + 79_200) * 7.0
        );
    }
    let rider = if cyber {
        // Warning lights on the three tallest towers.
        let mut tall: Vec<(f64, f64, f64)> = (0..30).map(tower).collect();
        tall.sort_by(|a, b| b.2.total_cmp(&a.2));
        let mut s = String::from(
            r##"<g class="va-drone d1"><circle r="3"/><path d="M-9 0 L 9 0"/></g><g class="va-drone d2"><circle r="2.4"/><path d="M-7 0 L 7 0"/></g><g class="va-beacons">"##,
        );
        for (k, (x, w, h)) in tall.into_iter().take(3).enumerate() {
            let _ = write!(
                s,
                r##"<circle cx="{:.0}" cy="{:.0}" r="2.6" style="animation-delay:-{:.1}s"/>"##,
                x + w / 2.0,
                638.0 - h,
                k as f64 * 0.7
            );
        }
        s.push_str("</g>");
        s
    } else {
        format!(
            r##"<g class="va-boat"><g class="va-bob">
  <ellipse class="va-boat-shade" cx="0" cy="6" rx="46" ry="5"/>
  <path class="va-hull" d="M-40 -6 Q -30 10 0 10 Q 30 10 44 -8 L 30 -4 L -28 -4 Z"/>
  <path class="va-hull-hi" d="M-40 -6 L -28 -4 L 30 -4 L 44 -8 L 30 -1 L -28 -1 Z"/>
  {}
  <path class="va-oar" d="M10 -8 L 34 12"/>
</g></g>"##,
            sitting_cat(-6.0, -4.0, 0.36, false)
        )
    };
    let body = format!(
        r##"{twinkle}
<g class="w-flock-path">{flock}</g>
<g><g clip-path="url(#va-fall-clip)"><g class="va-fall">{drops}</g></g><ellipse class="va-fall-foam" cx="948" cy="620" rx="24" ry="7"/></g>
<g class="va-flow slow">{far}</g>
<g class="va-flow fast">{near}</g>
<g class="va-drift">{drift}</g>
<g class="va-glints">{glints}</g>
{rider}"##,
        flock = birds(8100, 7, 300.0),
        far = flow(72_000, 648.0, 730.0, 16, 400.0),
        near = flow(72_400, 730.0, 900.0, 18, 400.0),
    );
    let defs = r##"<clipPath id="va-fall-clip"><rect x="930" y="332" width="30" height="286"/></clipPath>"##;
    layer("va-life", "xMidYMax", cyber, defs, &body)
}

fn pagoda() -> String {
    let mut posts = String::new();
    for k in 0..7 {
        let x = 1232.0 + f64::from(k) * 32.0;
        let _ = write!(posts, r##"<rect class="va-wood" x="{x:.0}" y="518" width="7" height="72"/>"##);
    }
    let mut rail = String::new();
    for k in 0..25 {
        let x = 1226.0 + f64::from(k) * 8.6;
        let _ = write!(rail, r##"<rect class="va-wood" x="{x:.1}" y="566" width="2" height="22"/>"##);
    }
    let mut upper_rail = String::new();
    for k in 0..15 {
        let x = 1266.0 + f64::from(k) * 9.0;
        let _ = write!(upper_rail, r##"<rect class="va-wood" x="{x:.1}" y="452" width="2" height="15"/>"##);
    }
    format!(
        r##"<g class="va-pagoda">
  <rect class="va-stone" x="1206" y="588" width="248" height="12" rx="2"/><rect class="va-stone dark" x="1196" y="598" width="268" height="8" rx="2"/>
  <rect class="va-wall" x="1252" y="520" width="156" height="68"/>
  <rect class="va-win" x="1266" y="530" width="38" height="46" rx="2"/><rect class="va-win" x="1312" y="530" width="38" height="46" rx="2"/><rect class="va-win" x="1358" y="530" width="38" height="46" rx="2"/>
  <path class="va-wood" d="M1285 530 V 576 M1331 530 V 576 M1377 530 V 576 M1266 553 H 1396" fill="none"/>
  {posts}
  <rect class="va-wood" x="1222" y="564" width="216" height="3"/><rect class="va-wood" x="1222" y="576" width="216" height="2"/>{rail}
  <path class="va-roof" d="M1150 500 Q 1210 500 1252 466 L 1408 466 Q 1450 500 1510 500 Q 1456 514 1418 520 L 1242 520 Q 1204 514 1150 500 Z"/>
  <path class="va-roof-shade" d="M1150 500 Q 1204 514 1242 520 L 1418 520 Q 1456 514 1510 500 Q 1450 508 1414 510 L 1246 510 Q 1210 508 1150 500 Z"/>
  <path class="va-roof-edge glow" d="M1150 500 Q 1210 500 1252 466 L 1408 466 Q 1450 500 1510 500"/><path class="va-roof-edge" d="M1150 500 Q 1210 500 1252 466 L 1408 466 Q 1450 500 1510 500"/>
  <rect class="va-wall" x="1272" y="416" width="116" height="52"/>
  <rect class="va-win" x="1284" y="426" width="26" height="28" rx="2"/><rect class="va-win" x="1317" y="426" width="26" height="28" rx="2"/><rect class="va-win" x="1350" y="426" width="26" height="28" rx="2"/>
  <rect class="va-wood" x="1262" y="450" width="136" height="3"/>{upper_rail}
  <path class="va-roof" d="M1206 404 Q 1260 402 1292 370 L 1368 370 Q 1400 402 1454 404 Q 1404 416 1374 420 L 1286 420 Q 1256 416 1206 404 Z"/>
  <path class="va-roof-shade" d="M1206 404 Q 1256 416 1286 420 L 1374 420 Q 1404 416 1454 404 Q 1400 410 1370 411 L 1290 411 Q 1260 410 1206 404 Z"/>
  <path class="va-roof-edge glow" d="M1206 404 Q 1260 402 1292 370 L 1368 370 Q 1400 402 1454 404"/><path class="va-roof-edge" d="M1206 404 Q 1260 402 1292 370 L 1368 370 Q 1400 402 1454 404"/>
  <path class="va-roof" d="M1290 372 Q 1320 352 1330 314 Q 1340 352 1370 372 Z"/>
  <path class="va-roof-edge glow" d="M1290 372 Q 1320 352 1330 314 Q 1340 352 1370 372"/><path class="va-roof-edge" d="M1290 372 Q 1320 352 1330 314 Q 1340 352 1370 372"/>
  <rect class="va-wood" x="1328" y="284" width="4" height="34"/><circle class="va-wood" cx="1330" cy="296" r="5"/><circle class="va-wood" cx="1330" cy="282" r="3"/>
</g>"##
    )
}

fn island(cyber: bool) -> String {
    let defs = format!(
        "{}{}{}",
        grad2("va-rockg", "--va-rock-hi", "--va-rock"),
        soft("va-wing", "--va-win", 0.75),
        soft("va-glow-i", "--va-b1", 0.55),
    );
    let life = if cyber { island_life() } else { "" };
    let mut back_pines = String::new();
    for (i, (x, y, h)) in [
        (1100.0, 640.0, 96.0),
        (1140.0, 628.0, 130.0),
        (1186.0, 612.0, 150.0),
        (1236.0, 600.0, 120.0),
        (1470.0, 600.0, 170.0),
        (1512.0, 596.0, 132.0),
    ]
    .into_iter()
    .enumerate()
    {
        back_pines.push_str(&pine(x, y, h, i % 2 == 0));
    }
    let body = format!(
        r##"<g class="va-island-shade"><path d="M1000 730 Q 1200 700 1800 720 L 1800 840 Q 1400 850 1180 800 Q 1050 780 1000 730 Z"/></g>
<path class="va-trunk-s" style="stroke-width:10" d="M1196 606 C 1190 560 1200 520 1180 480 M1192 540 C 1170 520 1150 510 1130 500"/>
<path class="va-trunk-s" style="stroke-width:12" d="M1458 600 C 1464 540 1450 480 1470 430 M1462 520 C 1500 490 1530 480 1560 450"/>
<g class="va-bloom-glow"><ellipse cx="1160" cy="470" rx="190" ry="130" fill="url(#va-glow-i)"/><ellipse cx="1500" cy="400" rx="300" ry="190" fill="url(#va-glow-i)"/></g>
{b1}{b2}{b3}{b4}
<g class="grn">{g0}</g>
{back_pines}
<path fill="url(#va-rockg)" d="M1010 712 Q 1030 670 1090 654 Q 1130 612 1200 602 L 1470 598 Q 1560 592 1800 556 L 1800 800 Q 1560 806 1420 774 Q 1300 792 1180 758 Q 1070 752 1010 712 Z"/>
<path class="va-rock-lit" d="M1040 700 L 1094 662 L 1128 700 L 1080 738 Z M1150 690 L 1230 650 L 1260 700 L 1200 744 Z M1290 700 L 1380 660 L 1420 716 L 1330 760 Z"/>
<path class="va-rock-dark" d="M1128 700 L 1150 690 L 1200 744 L 1180 758 Z M1260 700 L 1290 700 L 1330 760 L 1300 770 Z M1420 716 L 1470 690 L 1520 760 L 1420 774 Z"/>
<path class="va-moss" d="M1090 654 Q 1130 612 1200 602 L 1470 598 Q 1560 592 1800 556 L 1800 596 Q 1560 622 1470 626 L 1210 632 Q 1150 642 1090 654 Z"/>
<path class="va-moss hi" d="M1130 624 Q 1170 606 1210 604 L 1460 600 Q 1540 596 1640 580 Q 1540 606 1460 610 L 1220 614 Q 1170 616 1130 624 Z"/>
<g class="grn">{g1}{g2}{g3}</g>
<g class="va-win-glow"><ellipse cx="1330" cy="552" rx="150" ry="70" fill="url(#va-wing)"/><ellipse cx="1330" cy="440" rx="100" ry="46" fill="url(#va-wing)"/></g>
{pagoda}
{lamp}
<g class="grn">{g4}</g>
{r1}{r2}{r3}
<path class="va-rock near" d="M1490 960 L 1506 720 Q 1524 580 1604 520 L 1800 480 L 1800 960 Z"/>
<path class="va-rock-dark" d="M1506 720 Q 1560 700 1600 760 L 1620 960 L 1490 960 Z"/>
<path class="va-moss" d="M1506 720 Q 1524 580 1604 520 L 1800 480 L 1800 520 L 1622 552 Q 1552 606 1536 716 Z"/>
<path class="va-moss hi" d="M1560 600 Q 1590 548 1640 528 L 1800 496 L 1800 506 L 1646 540 Q 1600 560 1560 600 Z"/>
{life}"##,
        b1 = bloom(110_000, 1160.0, 470.0, 120.0, 70.0, 80, 15.0),
        b2 = bloom(111_000, 1480.0, 420.0, 150.0, 86.0, 110, 16.0),
        b3 = bloom(112_000, 1620.0, 330.0, 150.0, 90.0, 90, 17.0),
        b4 = bloom(113_000, 1060.0, 540.0, 70.0, 40.0, 40, 12.0),
        g0 = bloom(114_000, 1290.0, 500.0, 90.0, 60.0, 50, 15.0),
        g1 = bloom(115_000, 1126.0, 628.0, 46.0, 22.0, 26, 10.0),
        g2 = bloom(116_000, 1500.0, 590.0, 60.0, 26.0, 30, 11.0),
        g3 = bloom(117_000, 1640.0, 566.0, 70.0, 30.0, 34, 12.0),
        g4 = bloom(118_000, 1196.0, 612.0, 34.0, 16.0, 18, 9.0),
        pagoda = pagoda(),
        lamp = lantern(1176.0, 606.0, 0.55),
        r1 = water_rock(1066.0, 742.0, 70.0),
        r2 = water_rock(1250.0, 792.0, 96.0),
        r3 = water_rock(1404.0, 800.0, 60.0),
    );
    layer("va-island w-layer mid", "xMaxYMax", cyber, &defs, &body)
}

/// Cyber only: a robot cat patrolling in front of the pagoda, and a flickering holo sign.
fn island_life() -> &'static str {
    r##"<g class="va-sign"><rect x="1524" y="470" width="34" height="96" rx="4"/><text x="1541" y="502">桜</text><text x="1541" y="536">茶</text><path d="M1541 566 V 596"/></g>
<g class="walker va-walker"><g class="walker-flip"><g class="walk-cat">
  <path class="wc-tail" d="M-18 -18 C -32 -22 -34 -38 -26 -44"/>
  <ellipse class="wc-body" cx="0" cy="-16" rx="20" ry="10"/>
  <circle class="wc-head" cx="20" cy="-28" r="10"/>
  <path class="wc-ear" d="M13 -35 L 14 -46 L 20 -38 Z M23 -37 L 28 -46 L 29 -34 Z"/>
  <rect class="wc-visor" x="18" y="-31" width="12" height="4" rx="2"/>
  <path class="leg l1" d="M-12 -8 L -14 0"/><path class="leg l2" d="M-6 -8 L -4 0"/>
  <path class="leg l1" d="M8 -8 L 10 0"/><path class="leg l2" d="M14 -8 L 12 0"/>
</g></g></g>"##
}

fn left_bank(cyber: bool) -> String {
    let defs = format!(
        "{}{}",
        grad2("va-rockg2", "--va-rock-hi", "--va-rock"),
        grad2("va-mossg", "--va-moss-hi", "--va-moss")
    );
    let mut flowers = String::new();
    for i in 0..34u32 {
        let x = -60.0 + rnd(i + 120_000) * 480.0;
        // Follow the top of the boulder.
        let top = if x < 300.0 {
            648.0 - (x + 100.0) / 400.0 * 4.0 + (x / 300.0).powi(2) * 8.0
        } else {
            652.0 + (x - 300.0) * 0.5
        };
        let y = top + 6.0 + rnd(i + 121_000) * 34.0;
        let _ = write!(
            flowers,
            r##"<circle class="va-daisy" cx="{x:.0}" cy="{y:.0}" r="{:.1}"/>"##,
            1.8 + rnd(i + 122_000) * 1.8
        );
    }
    let mut grass = String::new();
    for i in 0..40u32 {
        let x = -80.0 + rnd(i + 123_000) * 500.0;
        let top = if x < 300.0 { 646.0 + (x / 300.0).powi(2) * 8.0 } else { 652.0 + (x - 300.0) * 0.5 };
        let h = 8.0 + rnd(i + 124_000) * 14.0;
        let _ = write!(
            grass,
            r##"<path class="va-grass" d="M{x:.0} {:.0} q {:.0} {:.0} {:.0} {:.0}"/>"##,
            top + 4.0,
            rnd(i + 125_000) * 6.0 - 3.0,
            -h * 0.6,
            rnd(i + 126_000) * 10.0 - 5.0,
            -h
        );
    }
    let body = format!(
        r##"<path class="va-bank" d="M-200 670 L -200 556 Q 150 516 340 558 Q 480 590 566 640 L 610 670 Z"/>
<path class="va-trunk-s" style="stroke-width:14" d="M120 610 C 126 560 110 520 128 470 M122 540 C 90 510 60 500 30 480 M126 520 C 160 490 190 480 220 450"/>
<path class="va-trunk-s" style="stroke-width:11" d="M400 620 C 404 580 392 560 406 520 M402 570 C 430 548 450 544 476 530"/>
{b1}{b2}{b3}
<g class="grn">{g1}{g2}</g>
<path fill="url(#va-rockg2)" d="M430 960 Q 450 866 540 842 Q 640 818 730 852 Q 820 886 860 960 Z"/>
<path class="va-rock-lit" d="M470 900 L 540 852 L 600 880 L 560 960 L 480 960 Z"/>
<path class="va-moss" d="M450 880 Q 470 850 540 842 Q 640 818 730 852 Q 790 874 820 910 Q 720 866 620 858 Q 520 852 450 880 Z"/>
<path class="va-moss hi" d="M520 846 Q 620 824 716 852 Q 620 838 520 846 Z"/>
<path fill="url(#va-rockg2)" d="M-200 960 L -200 644 Q 120 620 300 652 Q 400 670 452 742 Q 500 792 580 818 Q 700 852 780 960 Z"/>
<path class="va-rock-lit" d="M150 720 Q 250 716 330 776 Q 380 830 400 960 L 240 960 Q 232 820 150 720 Z"/>
<path class="va-rock-dark" d="M-200 760 Q -60 740 60 790 Q 150 840 170 960 L -200 960 Z M400 960 Q 392 850 452 742 Q 500 792 580 818 Q 520 830 500 960 Z"/>
<path class="va-crack" d="M210 730 Q 250 800 244 900 M330 776 Q 300 830 320 900 M60 790 Q 90 840 80 920"/>
<path fill="url(#va-mossg)" d="M-200 644 Q 120 620 300 652 Q 400 670 452 742 Q 400 716 330 708 Q 230 694 110 706 Q -40 716 -200 742 Z"/>
<path class="va-moss hi" d="M-120 648 Q 120 630 290 656 Q 360 668 400 700 Q 320 672 220 668 Q 60 660 -120 672 Z"/>
<path class="va-moss drip" d="M110 706 Q 120 760 100 800 Q 86 760 90 708 Z M250 696 Q 270 750 256 786 Q 236 750 236 696 Z M352 712 Q 372 752 366 790 Q 344 760 338 710 Z"/>
{grass}{flowers}
{cat}"##,
        cat = sitting_cat(352.0, 664.0, 0.78, !cyber),
        b1 = bloom(130_000, 120.0, 430.0, 170.0, 96.0, 130, 17.0),
        b2 = bloom(131_000, 420.0, 488.0, 120.0, 62.0, 90, 14.0),
        b3 = bloom(132_000, 270.0, 566.0, 80.0, 34.0, 44, 11.0),
        g1 = bloom(133_000, 30.0, 600.0, 110.0, 40.0, 50, 14.0),
        g2 = bloom(134_000, 520.0, 628.0, 70.0, 24.0, 32, 10.0),
    );
    layer("va-left w-layer near", "xMinYMax", cyber, &defs, &body)
}

fn rays(cyber: bool) -> String {
    let defs = r##"<linearGradient id="va-ray" x1="1" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".6"/><stop offset=".8" stop-color="#fff" stop-opacity="0"/></linearGradient>"##;
    let body = r##"<path d="M1580 -700 L 1790 -700 L 700 960 L 400 960 Z"/><path d="M1320 -700 L 1420 -700 L 330 960 L 190 960 Z"/><path d="M1840 -700 L 1950 -700 L 1060 960 L 920 960 Z"/>"##;
    layer("va-rays", "xMidYMax", cyber, defs, body)
}

fn canopy_left(cyber: bool) -> String {
    let defs = soft("va-glow-l", "--va-b1", 0.55);
    let body = format!(
        r##"<g class="va-bloom-glow"><ellipse cx="300" cy="60" rx="620" ry="250" fill="url(#va-glow-l)"/><ellipse cx="300" cy="330" rx="430" ry="190" fill="url(#va-glow-l)"/></g>
<path class="va-trunk" d="M-160 600 C -60 500 0 390 36 262 C 60 170 88 80 104 -80 L 214 -80 C 180 60 150 170 128 284 C 104 410 40 540 -40 680 Z"/>
<path class="va-trunk-hi" d="M196 -80 C 164 60 134 170 112 284 C 92 390 44 500 -20 620"/>
<path class="va-bark" d="M70 240 C 86 180 100 120 116 40 M50 330 C 70 280 80 250 92 200 M10 440 C 40 390 56 350 70 300"/>
<path class="va-branch" style="stroke-width:18" d="M112 190 C 220 132 330 150 450 112 C 540 84 620 92 720 56"/>
<path class="va-branch" style="stroke-width:14" d="M70 336 C 180 300 280 330 380 320 C 450 314 510 340 580 376"/>
<path class="va-branch" style="stroke-width:8" d="M330 148 C 360 200 390 220 420 262 M450 112 C 470 60 500 30 520 -30 M280 326 C 300 380 320 400 330 446 M600 86 C 640 110 670 140 690 180"/>
{c1}{c2}{c3}{c4}{c5}{c6}{c7}{c8}{c9}
{f1}{f2}{f3}{f4}{f5}"##,
        c1 = bloom(140_000, 150.0, 30.0, 270.0, 110.0, 170, 23.0),
        c2 = bloom(141_000, 480.0, 40.0, 220.0, 86.0, 130, 21.0),
        c3 = bloom(142_000, 720.0, 20.0, 150.0, 66.0, 80, 19.0),
        c4 = bloom(143_000, 90.0, 200.0, 150.0, 70.0, 80, 22.0),
        c5 = bloom(144_000, 250.0, 268.0, 150.0, 52.0, 70, 19.0),
        c6 = bloom(145_000, 450.0, 318.0, 140.0, 50.0, 70, 18.0),
        c7 = bloom(146_000, 590.0, 392.0, 80.0, 38.0, 40, 15.0),
        c8 = bloom(147_000, 40.0, 400.0, 150.0, 80.0, 80, 21.0),
        c9 = bloom(148_000, 336.0, 452.0, 60.0, 32.0, 30, 14.0),
        f1 = flower(676.0, 196.0, 15.0, 10.0),
        f2 = flower(640.0, 410.0, 13.0, 40.0),
        f3 = flower(424.0, 276.0, 12.0, 70.0),
        f4 = flower(704.0, 110.0, 12.0, 25.0),
        f5 = flower(318.0, 494.0, 11.0, 55.0),
    );
    layer("va-canopy va-canopy-l", "xMinYMin", cyber, &defs, &body)
}

fn canopy_right(cyber: bool) -> String {
    let defs = soft("va-glow-r", "--va-b1", 0.55);
    let body = format!(
        r##"<g class="va-bloom-glow"><ellipse cx="1520" cy="60" rx="420" ry="190" fill="url(#va-glow-r)"/><ellipse cx="1600" cy="260" rx="280" ry="200" fill="url(#va-glow-r)"/></g>
<path class="va-branch" style="stroke-width:16" d="M1800 30 C 1660 62 1540 50 1440 92 C 1380 114 1320 100 1250 66"/>
<path class="va-branch" style="stroke-width:12" d="M1800 226 C 1700 238 1620 250 1560 290 C 1530 310 1500 312 1462 334"/>
<path class="va-branch" style="stroke-width:7" d="M1440 92 C 1430 130 1400 160 1386 204 M1560 290 C 1570 330 1560 350 1570 384 M1330 92 C 1310 120 1300 130 1280 150"/>
{c1}{c2}{c3}{c4}{c5}{c6}
{f1}{f2}{f3}{f4}{f5}{f6}"##,
        c1 = bloom(150_000, 1570.0, 30.0, 230.0, 76.0, 120, 21.0),
        c2 = bloom(151_000, 1330.0, 76.0, 110.0, 50.0, 60, 18.0),
        c3 = bloom(152_000, 1660.0, 200.0, 140.0, 70.0, 80, 20.0),
        c4 = bloom(153_000, 1500.0, 304.0, 80.0, 40.0, 40, 16.0),
        c5 = bloom(154_000, 1396.0, 196.0, 56.0, 32.0, 28, 14.0),
        c6 = bloom(155_000, 1580.0, 392.0, 50.0, 28.0, 22, 13.0),
        f1 = flower(1250.0, 66.0, 15.0, 20.0),
        f2 = flower(1284.0, 152.0, 13.0, 60.0),
        f3 = flower(1456.0, 340.0, 14.0, 5.0),
        f4 = flower(1374.0, 236.0, 12.0, 35.0),
        f5 = flower(1226.0, 100.0, 10.0, 80.0),
        f6 = flower(1420.0, 300.0, 10.0, 48.0),
    );
    layer("va-canopy va-canopy-r", "xMaxYMin", cyber, &defs, &body)
}

/// The whole scene, back to front.
pub fn valley(cyber: bool) -> String {
    let mut s = String::with_capacity(400_000);
    s.push_str(&sky(cyber));
    s.push_str(&clouds(cyber));
    s.push_str(&mountains(cyber));
    s.push_str(&water_life(cyber));
    s.push_str(&island(cyber));
    s.push_str(&left_bank(cyber));
    s.push_str(&rays(cyber));
    s.push_str(&canopy_left(cyber));
    s.push_str(&canopy_right(cyber));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valley_is_well_formed_svg() {
        for cyber in [false, true] {
            let s = valley(cyber);
            assert!(s.starts_with("<svg") && s.ends_with("</svg>"));
            for tag in ["<svg", "<g", "<defs", "<text", "<clipPath", "<linearGradient", "<radialGradient"] {
                let open = s.matches(&format!("{tag} ")).count() + s.matches(&format!("{tag}>")).count();
                let close = s.matches(&format!("</{}>", &tag[1..])).count();
                assert_eq!(open, close, "cyber={cyber}: unbalanced {tag} ({open} vs {close})");
            }
            assert!(!s.contains("NaN") && !s.contains("inf"), "bad numbers");
            assert!(!s.contains(" --") && !s.contains("\"--"), "doubled minus sign");
            assert!(s.len() < 900_000, "scene is too heavy: {} bytes", s.len());
            // Filters are recomputed every frame once a layer moves; soft edges are gradients.
            assert!(!s.contains("<filter") && !s.contains("filter="), "no SVG filters");
        }
    }

    #[test]
    fn cyber_valley_has_its_own_life() {
        assert!(valley(true).contains("va-walker") && !valley(false).contains("va-walker"));
        assert!(valley(false).contains("va-boat") && !valley(true).contains("va-boat"));
    }
}
