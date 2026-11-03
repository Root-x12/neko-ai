//! Side panel that previews HTML, SVG, Mermaid and React from the chat in a sandboxed frame.

use leptos::prelude::*;
use neko_shared::MsgRole;
use wasm_bindgen::{JsCast, JsValue};

use crate::{
    icons::{self, Icon},
    markdown::{self, ArtifactKind},
    state::Ctx,
    util,
};

/// Every artifact in the open chat: (message id, index within message, kind).
fn all_artifacts(ctx: Ctx) -> Vec<(i64, usize, ArtifactKind)> {
    ctx.messages.with(|ms| {
        ms.iter()
            .filter(|m| m.role == MsgRole::Assistant && !m.streaming.get())
            .flat_map(|m| {
                let id = m.id;
                markdown::artifacts(&m.content.get()).into_iter().enumerate().map(move |(i, a)| (id, i, a.kind))
            })
            .collect()
    })
}

fn post_render(frame: &web_sys::HtmlIFrameElement, kind: ArtifactKind, code: &str, dark: bool) {
    let Some(win) = frame.content_window() else { return };
    let msg = js_sys::Object::new();
    let set = |k: &str, v: JsValue| {
        let _ = js_sys::Reflect::set(&msg, &JsValue::from_str(k), &v);
    };
    set("type", JsValue::from_str("neko-render"));
    set("kind", JsValue::from_str(kind.as_str()));
    set("code", JsValue::from_str(code));
    set("dark", JsValue::from_bool(dark));
    // The frame has an opaque origin, so "*" is the only target that reaches it.
    let _ = win.post_message(&msg, "*");
}

#[component]
pub fn Canvas() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let list = Memo::new(move |_| all_artifacts(ctx));
    let position = move || {
        ctx.canvas.with(|c| {
            c.as_ref().and_then(|c| list.with(|l| l.iter().position(|(m, i, _)| *m == c.msg_id && *i == c.index)))
        })
    };
    let step = move |delta: isize| {
        let Some(pos) = position() else { return };
        let next = pos as isize + delta;
        if let Some((m, i, _)) = list.with(|l| l.get(next.max(0) as usize).copied()) {
            if next >= 0 {
                ctx.open_artifact(m, i);
            }
        }
    };
    let close = move |_| {
        ctx.canvas.set(None);
        ctx.canvas_full.set(false);
    };
    let copy = move |_| {
        if let Some(c) = ctx.canvas.get_untracked() {
            util::copy_text(&c.code);
            ctx.notify("Code copied", false);
        }
    };
    let download = move |_| {
        if let Some(c) = ctx.canvas.get_untracked() {
            let mime = match c.kind {
                ArtifactKind::Html => "text/html",
                ArtifactKind::Svg => "image/svg+xml",
                _ => "text/plain",
            };
            util::download(&format!("artifact.{}", c.kind.file_ext()), mime, &c.code);
        }
    };
    // Highlight the code view whenever it is shown.
    Effect::new(move |_| {
        if ctx.canvas_code.get() && ctx.canvas.with(|c| c.is_some()) {
            util::next_frame(util::highlight_code);
        }
    });

    view! {
        <Show when=move || ctx.canvas.with(|c| c.is_some())>
            <aside class="canvas" class:full=move || ctx.canvas_full.get() aria-label="Preview">
                <header class="canvas-bar">
                    <div class="canvas-title">
                        <Icon svg=icons::PLAY size=16 />
                        <strong>{move || ctx.canvas.with(|c| c.as_ref().map(|c| c.kind.label()).unwrap_or(""))}</strong>
                        <Show when=move || { list.with(|l| l.len()) > 1 }>
                            <span class="canvas-nav">
                                <button
                                    class="icon-btn small"
                                    aria-label="Previous artifact"
                                    disabled=move || position() == Some(0)
                                    on:click=move |_| step(-1)
                                >
                                    "‹"
                                </button>
                                <span class="muted small">
                                    {move || format!("{} / {}", position().map_or(0, |p| p + 1), list.with(|l| l.len()))}
                                </span>
                                <button
                                    class="icon-btn small"
                                    aria-label="Next artifact"
                                    disabled=move || position().map(|p| p + 1) == Some(list.with(|l| l.len()))
                                    on:click=move |_| step(1)
                                >
                                    "›"
                                </button>
                            </span>
                        </Show>
                    </div>
                    <div class="segmented small">
                        <button class="seg" class:active=move || !ctx.canvas_code.get() on:click=move |_| ctx.canvas_code.set(false)>
                            "Preview"
                        </button>
                        <button class="seg" class:active=move || ctx.canvas_code.get() on:click=move |_| ctx.canvas_code.set(true)>
                            "Code"
                        </button>
                    </div>
                    <div class="canvas-actions">
                        <button class="icon-btn small" title="Copy code" aria-label="Copy code" on:click=copy>
                            <Icon svg=icons::COPY size=15 />
                        </button>
                        <button class="icon-btn small" title="Download" aria-label="Download" on:click=download>
                            <Icon svg=icons::DOWNLOAD size=15 />
                        </button>
                        <button
                            class="icon-btn small only-desktop"
                            title="Full screen"
                            aria-label="Toggle full screen"
                            on:click=move |_| ctx.canvas_full.update(|f| *f = !*f)
                        >
                            <Icon svg=icons::EXPAND size=15 />
                        </button>
                        <button class="icon-btn small" title="Close" aria-label="Close preview" on:click=close>
                            <Icon svg=icons::X size=16 />
                        </button>
                    </div>
                </header>
                <div class="canvas-body">
                    {move || {
                        let item = ctx.canvas.get()?;
                        if ctx.canvas_code.get() {
                            let class = format!("language-{}", item.kind.code_lang());
                            return Some(
                                view! {
                                    <div class="md canvas-code">
                                        <pre>
                                            <code class=class>{item.code}</code>
                                        </pre>
                                    </div>
                                }
                                    .into_any(),
                            );
                        }
                        // A fresh URL per render makes the frame reload, so each preview starts clean
                        // (the element itself may be reused).
                        let dark = ctx.theme.is_dark();
                        let src = format!("/canvas.html?r={}&d={}", ctx.canvas_rev.get(), dark as u8);
                        let (kind, code) = (item.kind, item.code);
                        Some(
                            view! {
                                <iframe
                                    class="canvas-frame"
                                    class:dark=dark && matches!(kind, ArtifactKind::Svg | ArtifactKind::Mermaid)
                                    title="Artifact preview"
                                    src=src
                                    sandbox="allow-scripts allow-modals"
                                    referrerpolicy="no-referrer"
                                    on:load=move |ev| {
                                        if let Some(frame) = ev
                                            .target()
                                            .and_then(|t| t.dyn_into::<web_sys::HtmlIFrameElement>().ok())
                                        {
                                            post_render(&frame, kind, &code, dark);
                                        }
                                    }
                                ></iframe>
                            }
                                .into_any(),
                        )
                    }}
                </div>
            </aside>
        </Show>
    }
}
