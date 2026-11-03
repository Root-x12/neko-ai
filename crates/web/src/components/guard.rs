//! The "this looks sensitive" dialog shown when the server holds a message back.

use leptos::prelude::*;

use crate::{
    icons::{self, Icon},
    state::Ctx,
};

#[component]
pub fn GuardDialog() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let hold = move || ctx.guard.get();
    view! {
        <Show when=move || hold().is_some()>
            {move || {
                let h = hold()?;
                // Files can't be redacted, so redacting only resolves findings in the message text.
                let redactable = h.findings.iter().all(|f| f.location == "message");
                let blocked = h.blocked;
                let has_secret = h.findings.iter().any(|f| f.secret);
                Some(view! {
                    <div class="modal-scrim" on:click=move |_| ctx.dismiss_guard()></div>
                    <div class="dialog card" role="alertdialog" aria-modal="true" aria-labelledby="guard-title">
                        <div class="dialog-icon" class:blocked=blocked>
                            <Icon svg=icons::SHIELD size=26 />
                        </div>
                        <h3 id="guard-title">
                            {if blocked { "Your workspace blocks sending this" } else { "Hold on, this looks sensitive" }}
                        </h3>
                        <p class="muted">
                            {if has_secret {
                                "Neko checked your message before it left your server and found what look like credentials:"
                            } else {
                                "Neko checked your message before it left your server and found personal data:"
                            }}
                        </p>
                        <ul class="findings">
                            {h.findings.iter().map(|f| view! {
                                <li>
                                    <span class="finding-kind" class:secret=f.secret>{f.kind.clone()}</span>
                                    <code>{f.sample.clone()}</code>
                                    <span class="muted small">
                                        {if f.location == "message" { "in your message".to_string() } else { format!("in {}", f.location) }}
                                    </span>
                                </li>
                            }).collect_view()}
                        </ul>
                        <p class="muted small">
                            {if redactable {
                                "Redacting replaces each item with a placeholder like [REDACTED OPENAI API KEY], so the AI never sees it."
                            } else {
                                "Items inside attached files can't be redacted. Remove the file or send it anyway."
                            }}
                        </p>
                        <div class="dialog-actions">
                            <button class="btn ghost" on:click=move |_| ctx.dismiss_guard()>"Edit message"</button>
                            <Show when=move || !blocked>
                                <button class="btn ghost danger" on:click=move |_| ctx.resend_guarded(false)>"Send anyway"</button>
                            </Show>
                            <Show when=move || redactable>
                                <button class="btn primary" on:click=move |_| ctx.resend_guarded(true)>
                                    <Icon svg=icons::SHIELD size=15 />
                                    "Redact & send"
                                </button>
                            </Show>
                        </div>
                    </div>
                })
            }}
        </Show>
    }
}
