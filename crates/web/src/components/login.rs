use leptos::{prelude::*, task::spawn_local};
use neko_shared::*;

use crate::{api, components::mascot::Mascot, components::ThemeToggles};

#[component]
pub fn Login(on_login: Callback<User>) -> impl IntoView {
    let status = RwSignal::new(None::<AuthStatus>);
    let register = RwSignal::new(false);
    let username = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let setup_code = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);

    spawn_local(async move {
        match api::get::<AuthStatus>("/auth/status").await {
            Ok(s) => {
                register.set(s.needs_setup);
                status.set(Some(s));
            }
            Err(e) => error.set(Some(e.message)),
        }
    });

    let needs_setup = move || status.with(|s| s.as_ref().is_some_and(|s| s.needs_setup));
    let signup_open = move || status.with(|s| s.as_ref().is_some_and(|s| s.signup_open));

    let submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        let (u, p) = (username.get_untracked(), password.get_untracked());
        let is_register = register.get_untracked();
        let code = setup_code.get_untracked();
        spawn_local(async move {
            let result = if is_register {
                let body =
                    RegisterRequest { username: u, password: p, setup_code: Some(code).filter(|c| !c.is_empty()) };
                api::send::<_, User>("POST", "/auth/register", &body).await
            } else {
                api::send::<_, User>("POST", "/auth/login", &LoginRequest { username: u, password: p }).await
            };
            busy.set(false);
            match result {
                Ok(user) => on_login.run(user),
                Err(e) => error.set(Some(e.message)),
            }
        });
    };

    view! {
        <div class="login-page">
            <div class="login-toggles"><ThemeToggles /></div>
            <form class="login-card card" on:submit=submit>
                <Mascot state=Signal::derive(move || if busy.get() { "thinking" } else { "idle" }) big=true />
                <h1 class="brand-title">"Neko AI"</h1>
                <p class="muted login-sub">
                    {move || if needs_setup() {
                        "Welcome! Create the admin account to get started."
                    } else if register.get() {
                        "Create your account"
                    } else {
                        "Welcome back"
                    }}
                </p>
                <label class="field">
                    <span>"Username"</span>
                    <input
                        type="text"
                        autocomplete="username"
                        autocapitalize="none"
                        spellcheck="false"
                        required
                        prop:value=move || username.get()
                        on:input=move |ev| username.set(event_target_value(&ev))
                    />
                </label>
                <label class="field">
                    <span>"Password"</span>
                    <input
                        type="password"
                        autocomplete=move || if register.get() { "new-password" } else { "current-password" }
                        required
                        minlength=move || if register.get() { "8" } else { "1" }
                        prop:value=move || password.get()
                        on:input=move |ev| password.set(event_target_value(&ev))
                    />
                </label>
                <Show when=needs_setup>
                    <label class="field">
                        <span>"Setup code"</span>
                        <input
                            type="text"
                            autocapitalize="characters"
                            spellcheck="false"
                            placeholder="Shown in the server console"
                            required
                            prop:value=move || setup_code.get()
                            on:input=move |ev| setup_code.set(event_target_value(&ev))
                        />
                    </label>
                </Show>
                {move || error.get().map(|e| view! { <p class="form-error" role="alert">{e}</p> })}
                <button class="btn primary wide" type="submit" disabled=move || busy.get() || status.with(|s| s.is_none())>
                    {move || match (busy.get(), register.get()) {
                        (true, _) => "One moment…",
                        (false, true) => "Create account",
                        (false, false) => "Sign in",
                    }}
                </button>
                <Show when=move || signup_open() && !needs_setup()>
                    <button
                        class="btn ghost wide"
                        type="button"
                        on:click=move |_| {
                            register.update(|r| *r = !*r);
                            error.set(None);
                        }
                    >
                        {move || if register.get() { "I already have an account" } else { "Create an account" }}
                    </button>
                </Show>
            </form>
        </div>
    }
}
