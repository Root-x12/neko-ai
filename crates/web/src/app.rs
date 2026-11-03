use std::time::Duration;

use leptos::{prelude::*, task::spawn_local};
use neko_shared::User;
use wasm_bindgen::{closure::Closure, JsCast};

use crate::{
    api,
    components::{
        chat::ChatView,
        login::Login,
        mascot::{Backdrop, Mascot},
        params::ParamsPanel,
        settings::SettingsModal,
        sidebar::Sidebar,
    },
    state::Ctx,
    theme::Theme,
    util,
};

const IDLE_AFTER_MS: f64 = 90_000.0;

#[component]
pub fn App() -> impl IntoView {
    let theme = Theme::init();
    provide_context(theme);

    // None = still checking the session.
    let user = RwSignal::new(None::<Option<User>>);
    spawn_local(async move {
        match api::get::<User>("/auth/me").await {
            Ok(u) => user.set(Some(Some(u))),
            Err(_) => user.set(Some(None)),
        }
    });

    view! {
        <Backdrop />
        {move || match user.get() {
            None => view! {
                <div class="splash"><Mascot state=Signal::stored("thinking") big=true /></div>
            }
            .into_any(),
            Some(None) => view! { <Login on_login=Callback::new(move |u| user.set(Some(Some(u)))) /> }.into_any(),
            Some(Some(u)) => view! { <Main user=u on_signed_out=Callback::new(move |_| user.set(Some(None))) /> }.into_any(),
        }}
    }
}

#[component]
fn Main(user: User, on_signed_out: Callback<()>) -> impl IntoView {
    let theme = expect_context::<Theme>();
    let ctx = Ctx::new(user, theme);
    provide_context(ctx);
    ctx.load_providers();
    ctx.load_conversations();
    ctx.load_mcp_servers();

    Effect::new(move |_| {
        if ctx.signed_out.get() {
            on_signed_out.run(());
        }
    });

    // The cat dozes off when nobody has touched the app for a while.
    let last_activity = StoredValue::new(js_sys::Date::now());
    let doc = util::window().document().expect("document");
    let wake = Closure::<dyn Fn()>::new(move || {
        last_activity.set_value(js_sys::Date::now());
        if ctx.idle.get_untracked() {
            ctx.idle.set(false);
        }
    });
    for ev in ["pointerdown", "keydown", "wheel", "touchstart"] {
        let _ = doc.add_event_listener_with_callback(ev, wake.as_ref().unchecked_ref());
    }
    let tick = set_interval_with_handle(
        move || {
            if !ctx.streaming.get_untracked() && js_sys::Date::now() - last_activity.get_value() > IDLE_AFTER_MS {
                ctx.idle.set(true);
            }
        },
        Duration::from_secs(5),
    )
    .ok();
    // Keyboard shortcuts: Esc closes panels, Ctrl/Cmd+Shift+O starts a new chat.
    let keys = Closure::<dyn Fn(web_sys::KeyboardEvent)>::new(move |ev: web_sys::KeyboardEvent| {
        if ev.key() == "Escape" {
            ctx.settings.set(None);
            ctx.params_open.set(false);
            ctx.sidebar_open.set(false);
        } else if (ev.ctrl_key() || ev.meta_key()) && ev.shift_key() && ev.key().eq_ignore_ascii_case("o") {
            ev.prevent_default();
            ctx.new_chat();
        }
    });
    let _ = doc.add_event_listener_with_callback("keydown", keys.as_ref().unchecked_ref());

    let listeners = StoredValue::new_local(Some((wake, keys)));
    on_cleanup(move || {
        if let Some(t) = tick {
            t.clear();
        }
        if let Some((wake, keys)) = listeners.try_update_value(|l| l.take()).flatten() {
            let doc = util::window().document().expect("document");
            for ev in ["pointerdown", "keydown", "wheel", "touchstart"] {
                let _ = doc.remove_event_listener_with_callback(ev, wake.as_ref().unchecked_ref());
            }
            let _ = doc.remove_event_listener_with_callback("keydown", keys.as_ref().unchecked_ref());
        }
    });

    view! {
        <div class="app">
            <Sidebar />
            <ChatView />
            <ParamsPanel />
            <SettingsModal />
            {move || ctx.toast.get().map(|t| view! {
                <div class="toast" class:error=t.error role="status" on:click=move |_| ctx.toast.set(None)>{t.text}</div>
            })}
        </div>
    }
}
