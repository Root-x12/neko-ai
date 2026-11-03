//! Small DOM helpers.

use js_sys::{Function, Reflect};
use wasm_bindgen::{JsCast, JsValue};

pub fn window() -> web_sys::Window {
    web_sys::window().expect("window")
}

/// Copies text, falling back to `execCommand` where the async Clipboard API is unavailable
/// (it only exists on HTTPS/localhost, so phones on plain-HTTP LAN addresses need the fallback).
pub fn copy_text(text: &str) {
    let navigator = window().navigator();
    let has_clipboard = Reflect::get(&navigator, &JsValue::from_str("clipboard"))
        .map(|v| !v.is_undefined() && !v.is_null())
        .unwrap_or(false);
    if has_clipboard && window().is_secure_context() {
        let _ = navigator.clipboard().write_text(text);
        return;
    }
    let Some(doc) = window().document() else { return };
    let Ok(el) = doc.create_element("textarea") else { return };
    let ta: web_sys::HtmlTextAreaElement = el.unchecked_into();
    ta.set_value(text);
    let _ = ta.set_attribute("readonly", "");
    let _ = ta.set_attribute("style", "position:fixed;top:-1000px;opacity:0");
    if let Some(body) = doc.body() {
        let _ = body.append_child(&ta);
        ta.select();
        if let Ok(html_doc) = doc.clone().dyn_into::<web_sys::HtmlDocument>() {
            let _ = html_doc.exec_command("copy");
        }
        ta.remove();
    }
}

/// Runs highlight.js over code blocks that aren't highlighted yet (see `index.html`).
pub fn highlight_code() {
    let w = window();
    if let Ok(f) = Reflect::get(&w, &JsValue::from_str("nekoHighlight")) {
        if let Some(f) = f.dyn_ref::<Function>() {
            let _ = f.call0(&JsValue::NULL);
        }
    }
}

/// Defers `f` to the next animation frame (after Leptos has updated the DOM).
pub fn next_frame(f: impl FnOnce() + 'static) {
    leptos::prelude::request_animation_frame(f);
}

pub fn confirm(msg: &str) -> bool {
    window().confirm_with_message(msg).unwrap_or(false)
}

pub fn prompt(msg: &str, default: &str) -> Option<String> {
    window().prompt_with_message_and_default(msg, default).ok().flatten()
}

pub fn now_secs() -> i64 {
    (js_sys::Date::now() / 1000.0) as i64
}

pub fn ago(ts: i64) -> String {
    let d = (now_secs() - ts).max(0);
    match d {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", d / 60),
        3600..=86_399 => format!("{}h ago", d / 3600),
        _ => format!("{}d ago", d / 86_400),
    }
}

/// Short device label from a user agent string.
pub fn device_label(ua: &str) -> String {
    let os = [
        ("iPhone", "iPhone"),
        ("iPad", "iPad"),
        ("Android", "Android"),
        ("Windows", "Windows"),
        ("Mac OS", "Mac"),
        ("CrOS", "ChromeOS"),
        ("Linux", "Linux"),
    ]
    .iter()
    .find(|(k, _)| ua.contains(k))
    .map(|(_, v)| *v)
    .unwrap_or("Device");
    let browser = [("Edg/", "Edge"), ("Firefox/", "Firefox"), ("Chrome/", "Chrome"), ("Safari/", "Safari")]
        .iter()
        .find(|(k, _)| ua.contains(k))
        .map(|(_, v)| *v);
    match browser {
        Some(b) => format!("{b} on {os}"),
        None => os.to_string(),
    }
}

pub fn is_touch() -> bool {
    window().match_media("(pointer: coarse)").ok().flatten().is_some_and(|m| m.matches())
}

pub fn file_size(bytes: i64) -> String {
    match bytes {
        b if b < 1024 => format!("{b} B"),
        b if b < 1024 * 1024 => format!("{:.0} KB", b as f64 / 1024.0),
        b => format!("{:.1} MB", b as f64 / 1024.0 / 1024.0),
    }
}

/// Saves text as a file via a temporary object URL.
pub fn download(filename: &str, mime: &str, content: &str) {
    let parts = js_sys::Array::of1(&JsValue::from_str(content));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type(mime);
    let Ok(blob) = web_sys::Blob::new_with_str_sequence_and_options(&parts, &opts) else { return };
    let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) else { return };
    if let Some(a) = window().document().and_then(|d| d.create_element("a").ok()) {
        let a: web_sys::HtmlAnchorElement = a.unchecked_into();
        a.set_href(&url);
        a.set_download(filename);
        a.click();
    }
    let _ = web_sys::Url::revoke_object_url(&url);
}
