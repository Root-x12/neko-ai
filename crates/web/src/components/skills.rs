//! The Skills tab (admin only): import a GitHub-hosted security tool as a cyber "agent",
//! review what it will run, start/stop it under supervision, and wire its endpoint + playbook
//! into chats. Mirrors the server in `skills.rs`.

use leptos::{prelude::*, task::spawn_local};
use neko_shared::{Skill, SkillImport, SkillLog, SkillPatch, SkillStatus};

use crate::{
    api,
    icons::{self, Icon},
    state::Ctx,
};

fn status_label(s: SkillStatus) -> &'static str {
    match s {
        SkillStatus::Imported => "imported",
        SkillStatus::Installing => "installing…",
        SkillStatus::Ready => "ready",
        SkillStatus::Running => "running",
        SkillStatus::Failed => "failed",
        SkillStatus::Stopped => "stopped",
    }
}

#[component]
pub fn SkillsTab() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let repo = RwSignal::new(String::new());
    let git_ref = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    ctx.load_skills();

    let import = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let url = repo.get_untracked().trim().to_string();
        if url.is_empty() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            let body = SkillImport { repo_url: url, git_ref: git_ref.get_untracked().trim().to_string() };
            match api::send::<_, Skill>("POST", "/skills", &body).await {
                Ok(_) => {
                    repo.set(String::new());
                    git_ref.set(String::new());
                    ctx.notify("Cloned. Review its setup, then run it.", false);
                    ctx.load_skills();
                }
                Err(e) => ctx.error(e),
            }
            busy.set(false);
        });
    };

    view! {
        <h3>"Skills"</h3>
        <p class="muted small">
            "Bring a security tool from GitHub (like ScopeSentry) in as a cyber agent. Neko clones the repo, \
             you review what it runs, then it runs under Neko and connects over its HTTP/MCP endpoint. \
             Cloning and running happen on your server, so this is admin-only."
        </p>

        <form class="skill-import" on:submit=import>
            <input
                class="input mono"
                r#type="text"
                placeholder="https://github.com/owner/repo"
                prop:value=move || repo.get()
                on:input=move |e| repo.set(event_target_value(&e))
            />
            <input
                class="input mono narrow"
                r#type="text"
                placeholder="branch / tag (optional)"
                prop:value=move || git_ref.get()
                on:input=move |e| git_ref.set(event_target_value(&e))
            />
            <button class="btn primary" r#type="submit" prop:disabled=move || busy.get()>
                <Icon svg=icons::PLUS size=16 />
                {move || if busy.get() { "Cloning…" } else { "Import" }}
            </button>
        </form>

        <div class="skill-list">
            <For each=move || ctx.skills.get() key=|s| (s.id, s.status.as_str(), s.enabled) let:skill>
                <SkillCard skill=skill />
            </For>
            {move || ctx.skills.with(|s| s.is_empty()).then(|| view! {
                <p class="muted small empty-hint">"No skills yet. Import one above to get started."</p>
            })}
        </div>
    }
}

#[component]
fn SkillCard(skill: Skill) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let id = skill.id;
    let busy = RwSignal::new(false);
    let open = RwSignal::new(false);
    let logs = RwSignal::new(None::<Vec<String>>);
    let instructions = RwSignal::new(skill.manifest.instructions.clone());
    let endpoint = RwSignal::new(skill.manifest.endpoint.clone());

    let status = skill.status;
    let running = status == SkillStatus::Running;
    let has_mcp = !skill.manifest.mcp_path.trim().is_empty() && !skill.manifest.endpoint.trim().is_empty();

    let act = move |path: String| {
        busy.set(true);
        spawn_local(async move {
            match api::call("POST", &path).await {
                Ok(()) => ctx.load_skills(),
                Err(e) => ctx.error(e),
            }
            busy.set(false);
        });
    };
    let run = move |_| act(format!("/skills/{id}/run"));
    let stop = move |_| act(format!("/skills/{id}/stop"));

    let refresh_logs = move || {
        spawn_local(async move {
            if let Ok(l) = api::get::<SkillLog>(&format!("/skills/{id}/logs")).await {
                logs.set(Some(l.lines));
            }
        });
    };
    let toggle_logs = move |_| {
        if logs.with(Option::is_some) {
            logs.set(None);
        } else {
            refresh_logs();
        }
    };

    let toggle_enabled = move |ev: leptos::ev::Event| {
        let on = event_target_checked(&ev);
        spawn_local(async move {
            let body = SkillPatch { enabled: Some(on), ..Default::default() };
            if let Err(e) = api::call_with("PATCH", &format!("/skills/{id}"), &body).await {
                ctx.error(e);
            }
            ctx.load_skills();
        });
    };

    let save = move |_| {
        busy.set(true);
        spawn_local(async move {
            let body = SkillPatch {
                instructions: Some(instructions.get_untracked()),
                endpoint: Some(endpoint.get_untracked()),
                ..Default::default()
            };
            match api::call_with("PATCH", &format!("/skills/{id}"), &body).await {
                Ok(()) => {
                    ctx.notify("Saved.", false);
                    ctx.load_skills();
                }
                Err(e) => ctx.error(e),
            }
            busy.set(false);
        });
    };

    let name = skill.name.clone();
    let remove = move |_| {
        let name = name.clone();
        spawn_local(async move {
            if let Err(e) = api::call("DELETE", &format!("/skills/{id}")).await {
                ctx.error(e);
                return;
            }
            ctx.notify(format!("Removed “{name}”."), false);
            ctx.load_skills();
        });
    };

    let repo_url = skill.repo_url.clone();
    let setup = skill.manifest.setup.clone();
    let run_cmd = skill.manifest.run.clone();
    let status_class = status.as_str();

    view! {
        <div class="skill-card" class:running=running>
            <div class="skill-head">
                <span class=format!("skill-dot {status_class}")></span>
                <strong>{skill.name.clone()}</strong>
                <span class="skill-status">{status_label(status)}</span>
                <a class="skill-repo" href=repo_url.clone() target="_blank" rel="noreferrer noopener">
                    {repo_url.replace("https://github.com/", "")}
                </a>
                <span class="spacer"></span>
                <Show when=move || running>
                    <label class="skill-toggle" title="Use this skill's tools and playbook in chats">
                        <input r#type="checkbox" prop:checked=skill.enabled on:change=toggle_enabled />
                        <span>"Active in chats"</span>
                    </label>
                </Show>
            </div>

            {(!has_mcp && running).then(|| view! {
                <p class="skill-hint muted small">
                    "Running, but no MCP endpoint is set, so its tools aren't auto-wired. \
                     Add an endpoint and MCP path below to let the assistant call it."
                </p>
            })}

            <div class="skill-cmds">
                {(!setup.is_empty()).then(|| view! {
                    <div class="cmd"><span class="cmd-tag">"setup"</span><code>{setup.clone()}</code></div>
                })}
                {if run_cmd.is_empty() {
                    view! {
                        <div class="cmd"><span class="cmd-tag warn">"run"</span><code class="muted">"not set — edit below"</code></div>
                    }.into_any()
                } else {
                    view! {
                        <div class="cmd"><span class="cmd-tag">"run"</span><code>{run_cmd.clone()}</code></div>
                    }.into_any()
                }}
            </div>

            <div class="skill-actions">
                <Show
                    when=move || running
                    fallback=move || view! {
                        <button class="btn small" on:click=run prop:disabled=move || busy.get()>
                            <Icon svg=icons::PLAY size=14 />"Run"
                        </button>
                    }
                >
                    <button class="btn small" on:click=stop prop:disabled=move || busy.get()>
                        <Icon svg=icons::STOP size=14 />"Stop"
                    </button>
                </Show>
                <button class="btn small ghost" on:click=toggle_logs>
                    <Icon svg=icons::TERMINAL size=14 />
                    {move || if logs.with(Option::is_some) { "Hide log" } else { "Log" }}
                </button>
                <button class="btn small ghost" on:click=move |_| open.update(|o| *o = !*o)>
                    <Icon svg=icons::WRENCH size=14 />"Edit"
                </button>
                <span class="spacer"></span>
                <button class="btn small danger" on:click=remove><Icon svg=icons::TRASH size=14 />"Remove"</button>
            </div>

            {move || logs.get().map(|lines| view! {
                <div class="skill-log">
                    <div class="skill-log-bar">
                        <span class="muted small">"output"</span>
                        <button class="btn tiny ghost" on:click=move |_| refresh_logs()>"Refresh"</button>
                    </div>
                    <pre>{if lines.is_empty() { "(no output yet)".to_string() } else { lines.join("\n") }}</pre>
                </div>
            })}

            <Show when=move || open.get()>
                <div class="skill-edit">
                    <label class="field">
                        <span>"Playbook — how the assistant should use this tool"</span>
                        <textarea
                            rows="5"
                            prop:value=move || instructions.get()
                            on:input=move |e| instructions.set(event_target_value(&e))
                        ></textarea>
                    </label>
                    <label class="field">
                        <span>"HTTP endpoint (where the tool serves once running)"</span>
                        <input
                            class="input mono"
                            r#type="text"
                            placeholder="http://127.0.0.1:8787"
                            prop:value=move || endpoint.get()
                            on:input=move |e| endpoint.set(event_target_value(&e))
                        />
                    </label>
                    <p class="muted small">
                        "Setup and run commands come from the repo's neko-skill.json and are shown above. \
                         Edit them in the repo and re-import to change them."
                    </p>
                    <button class="btn primary small" on:click=save prop:disabled=move || busy.get()>"Save"</button>
                </div>
            </Show>
        </div>
    }
}
