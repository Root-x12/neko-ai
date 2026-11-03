//! Settings → Admin: workspace policies (admins and team leads), users and the audit log.

use leptos::{prelude::*, task::spawn_local};
use neko_shared::*;

use crate::{
    api,
    icons::{self, Icon},
    state::Ctx,
    util,
};

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Policies,
    Users,
    Audit,
}

#[component]
pub fn AdminPanel() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let is_admin = move || ctx.user.with(|u| u.role == Role::Admin);
    let section = RwSignal::new(Section::Policies);
    let workspaces = RwSignal::new(Vec::<Workspace>::new());
    let reload_ws = move || {
        spawn_local(async move {
            match api::get::<Vec<Workspace>>("/admin/workspaces").await {
                Ok(list) => workspaces.set(list),
                Err(e) => ctx.error(e),
            }
        })
    };
    reload_ws();
    let seg = move |s: Section, label: &'static str| {
        view! { <button class="seg" class:active=move || section.get() == s on:click=move |_| section.set(s)>{label}</button> }
    };
    view! {
        <h3>{move || if is_admin() { "Administration" } else { "Team settings" }}</h3>
        <p class="muted">
            {move || if is_admin() {
                "Workspaces group people and set their policies. Every change here is recorded in the audit log."
            } else {
                "You lead this workspace: set its policies and review its activity."
            }}
        </p>
        <div class="segmented section-tabs">
            {seg(Section::Policies, "Policies")}
            <Show when=is_admin>{seg(Section::Users, "Users")}</Show>
            {seg(Section::Audit, "Audit log")}
        </div>
        {move || match section.get() {
            Section::Policies => view! { <Policies workspaces=workspaces reload=Callback::new(move |_| reload_ws()) /> }.into_any(),
            Section::Users => view! { <Users workspaces=workspaces /> }.into_any(),
            Section::Audit => view! { <Audit workspaces=workspaces /> }.into_any(),
        }}
    }
}

// ---------- policies ----------

fn policy_select(value: RwSignal<GuardPolicy>) -> impl IntoView {
    view! {
        <select prop:value=move || value.get().as_str().to_string() on:change=move |ev| value.set(GuardPolicy::parse(&event_target_value(&ev)))>
            <option value="warn">"Warn and ask"</option>
            <option value="redact">"Redact automatically"</option>
            <option value="block">"Block"</option>
            <option value="off">"Off"</option>
        </select>
    }
}

#[component]
fn WorkspaceCard(w: Workspace, reload: Callback<()>) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let is_admin = ctx.user.with_untracked(|u| u.role == Role::Admin);
    let id = w.id;
    let name = RwSignal::new(w.name.clone());
    let secret = RwSignal::new(w.secret_policy);
    let pii = RwSignal::new(w.pii_policy);
    let injection = RwSignal::new(w.injection_scan);
    let retention = RwSignal::new(w.retention_days.map(|d| d.to_string()).unwrap_or_default());
    let allow = RwSignal::new(w.model_allowlist.join("\n"));
    let save = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let days = retention.get_untracked().trim().to_string();
        let retention_days = if days.is_empty() {
            None
        } else {
            match days.parse::<u32>() {
                Ok(d) => Some(d),
                Err(_) => return ctx.notify("Retention must be a number of days", true),
            }
        };
        let input = WorkspaceInput {
            name: name.get_untracked(),
            model_allowlist: allow.get_untracked().lines().map(str::to_string).collect(),
            retention_days,
            secret_policy: secret.get_untracked(),
            pii_policy: pii.get_untracked(),
            injection_scan: injection.get_untracked(),
        };
        spawn_local(async move {
            match api::send::<_, Workspace>("PUT", &format!("/admin/workspaces/{id}"), &input).await {
                Ok(w) => {
                    ctx.notify(format!("Saved {}", w.name), false);
                    reload.run(());
                    ctx.load_info();
                }
                Err(e) => ctx.error(e),
            }
        });
    };
    let delete = move |_| {
        if !util::confirm("Delete this workspace? It must have no members.") {
            return;
        }
        spawn_local(async move {
            match api::call("DELETE", &format!("/admin/workspaces/{id}")).await {
                Ok(()) => reload.run(()),
                Err(e) => ctx.error(e),
            }
        });
    };
    view! {
        <form class="card inset ws-card" on:submit=save>
            <div class="ws-head">
                <input class="ws-name" type="text" prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev)) aria-label="Workspace name" />
                <span class="muted small">{format!("{} member{}", w.member_count, if w.member_count == 1 { "" } else { "s" })}</span>
            </div>
            <div class="policy-grid">
                <label class="field">
                    <span>"Secrets (API keys, passwords)"</span>
                    {policy_select(secret)}
                </label>
                <label class="field">
                    <span>"Personal data (emails, cards…)"</span>
                    {policy_select(pii)}
                </label>
                <label class="field">
                    <span>"Delete chats after (days)"</span>
                    <input type="number" min="1" placeholder="Keep forever" prop:value=move || retention.get() on:input=move |ev| retention.set(event_target_value(&ev)) />
                </label>
                <label class="check">
                    <input type="checkbox" prop:checked=move || injection.get() on:change=move |ev| injection.set(event_target_checked(&ev)) />
                    "Scan pages, tool results & files for prompt injection"
                </label>
            </div>
            <label class="field">
                <span>"Allowed models (one pattern per line, * = wildcard; empty = all)"</span>
                <textarea rows="3" spellcheck="false" placeholder="claude-*\ngpt-4o*\nllama3*" prop:value=move || allow.get() on:input=move |ev| allow.set(event_target_value(&ev))></textarea>
            </label>
            <div class="form-actions">
                {(is_admin && id != 1).then(|| view! { <button class="btn ghost danger" type="button" on:click=delete>"Delete"</button> })}
                <button class="btn primary" type="submit">"Save policies"</button>
            </div>
        </form>
    }
}

#[component]
fn Policies(workspaces: RwSignal<Vec<Workspace>>, reload: Callback<()>) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let is_admin = move || ctx.user.with(|u| u.role == Role::Admin);
    let new_name = RwSignal::new(String::new());
    let create = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let input = WorkspaceInput {
            name: new_name.get_untracked(),
            model_allowlist: Vec::new(),
            retention_days: None,
            secret_policy: GuardPolicy::Warn,
            pii_policy: GuardPolicy::Warn,
            injection_scan: true,
        };
        spawn_local(async move {
            match api::send::<_, Workspace>("POST", "/admin/workspaces", &input).await {
                Ok(_) => {
                    new_name.set(String::new());
                    reload.run(());
                }
                Err(e) => ctx.error(e),
            }
        });
    };
    view! {
        <For each=move || workspaces.get() key=|w| format!("{w:?}") let:w>
            <WorkspaceCard w=w reload=reload />
        </For>
        <Show when=is_admin>
            <form class="inline-form" on:submit=create>
                <input type="text" placeholder="New workspace name" prop:value=move || new_name.get() on:input=move |ev| new_name.set(event_target_value(&ev)) />
                <button class="btn ghost" type="submit">
                    <Icon svg=icons::PLUS size=15 />
                    "Add workspace"
                </button>
            </form>
        </Show>
    }
}

// ---------- users ----------

#[component]
fn Users(workspaces: RwSignal<Vec<Workspace>>) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let users = RwSignal::new(Vec::<AdminUser>::new());
    let reload = move || {
        spawn_local(async move {
            match api::get::<Vec<AdminUser>>("/admin/users").await {
                Ok(list) => users.set(list),
                Err(e) => ctx.error(e),
            }
        })
    };
    reload();
    let update = move |id: i64, patch: UpdateUser| {
        spawn_local(async move {
            match api::send::<_, AdminUser>("PATCH", &format!("/admin/users/{id}"), &patch).await {
                Ok(_) => reload(),
                Err(e) => {
                    ctx.error(e);
                    reload();
                }
            }
        })
    };
    let remove = move |u: AdminUser| {
        if !util::confirm(&format!("Delete {} and all their chats, keys and files?", u.username)) {
            return;
        }
        spawn_local(async move {
            match api::call("DELETE", &format!("/admin/users/{}", u.id)).await {
                Ok(()) => reload(),
                Err(e) => ctx.error(e),
            }
        });
    };
    let reset = move |u: AdminUser| {
        let Some(pw) = util::prompt(&format!("New password for {} (at least 8 characters)", u.username), "") else {
            return;
        };
        update(u.id, UpdateUser { password: Some(pw), ..Default::default() });
    };

    let (nu, np, nr, nw) =
        (RwSignal::new(String::new()), RwSignal::new(String::new()), RwSignal::new(Role::User), RwSignal::new(1i64));
    let create = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let body = CreateUser {
            username: nu.get_untracked(),
            password: np.get_untracked(),
            role: nr.get_untracked(),
            workspace_id: nw.get_untracked(),
        };
        spawn_local(async move {
            match api::send::<_, AdminUser>("POST", "/admin/users", &body).await {
                Ok(u) => {
                    ctx.notify(format!("Created {}", u.username), false);
                    nu.set(String::new());
                    np.set(String::new());
                    reload();
                }
                Err(e) => ctx.error(e),
            }
        });
    };
    let ws_options = move |selected: i64| {
        workspaces
            .get()
            .into_iter()
            .map(|w| view! { <option value=w.id.to_string() selected=w.id == selected>{w.name}</option> })
            .collect_view()
    };
    let role_select = move |current: Role, on_change: Callback<Role>| {
        view! {
            <select on:change=move |ev| on_change.run(Role::parse(&event_target_value(&ev)))>
                {[Role::User, Role::Team, Role::Admin].into_iter().map(|r| view! {
                    <option value=r.as_str() selected=r == current>{r.label()}</option>
                }).collect_view()}
            </select>
        }
    };
    view! {
        <div class="user-table">
            <For each=move || users.get() key=|u| format!("{u:?}") let:u>
                {
                    let id = u.id;
                    let me = ctx.user.with_untracked(|m| m.id == id);
                    let (ur, ud) = (u.clone(), u.clone());
                    view! {
                        <div class="user-row" class:disabled=u.disabled>
                            <span class="avatar small">{u.username.chars().next().unwrap_or('?').to_uppercase().to_string()}</span>
                            <div class="user-main">
                                <strong>{u.username.clone()} {me.then_some(" (you)")}</strong>
                                <span class="muted small">"joined " {util::ago(u.created_at)} {u.disabled.then_some(" · disabled")}</span>
                            </div>
                            {role_select(u.role, Callback::new(move |r| update(id, UpdateUser { role: Some(r), ..Default::default() })))}
                            <select on:change=move |ev| {
                                if let Ok(w) = event_target_value(&ev).parse::<i64>() {
                                    update(id, UpdateUser { workspace_id: Some(w), ..Default::default() });
                                }
                            }>
                                {move || ws_options(u.workspace_id)}
                            </select>
                            <div class="row-actions">
                                <button class="btn small ghost" on:click=move |_| reset(ur.clone())>"Reset password"</button>
                                {(!me).then(|| view! {
                                    <button class="btn small ghost" on:click=move |_| update(id, UpdateUser { disabled: Some(!u.disabled), ..Default::default() })>
                                        {if u.disabled { "Enable" } else { "Disable" }}
                                    </button>
                                    <button class="btn small ghost danger" on:click=move |_| remove(ud.clone())>"Delete"</button>
                                })}
                            </div>
                        </div>
                    }
                }
            </For>
        </div>
        <form class="card inset" on:submit=create>
            <h4>"Add user"</h4>
            <div class="policy-grid">
                <label class="field"><span>"Username"</span><input type="text" autocomplete="off" required prop:value=move || nu.get() on:input=move |ev| nu.set(event_target_value(&ev)) /></label>
                <label class="field"><span>"Password"</span><input type="password" autocomplete="new-password" minlength="8" required prop:value=move || np.get() on:input=move |ev| np.set(event_target_value(&ev)) /></label>
                <label class="field"><span>"Role"</span>{role_select(Role::User, Callback::new(move |r| nr.set(r)))}</label>
                <label class="field">
                    <span>"Workspace"</span>
                    <select on:change=move |ev| {
                        if let Ok(w) = event_target_value(&ev).parse::<i64>() {
                            nw.set(w)
                        }
                    }>{move || ws_options(nw.get_untracked())}</select>
                </label>
            </div>
            <div class="form-actions"><button class="btn primary" type="submit">"Create user"</button></div>
        </form>
    }
}

// ---------- audit ----------

const AUDIT_FILTERS: &[(&str, &str)] = &[
    ("", "Everything"),
    ("auth.", "Sign-ins"),
    ("guard.", "Guard (secrets & injection)"),
    ("admin.", "Admin changes"),
    ("provider.", "Providers"),
    ("mcp.", "MCP servers"),
    ("chat.", "Chats"),
    ("device.", "Devices"),
];

fn csv_field(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

#[component]
fn Audit(workspaces: RwSignal<Vec<Workspace>>) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let entries = RwSignal::new(Vec::<AuditEntry>::new());
    let filter = RwSignal::new(String::new());
    let more = RwSignal::new(true);
    let load = move |append: bool| {
        let before = if append { entries.with_untracked(|e| e.last().map(|x| x.id)) } else { None };
        let mut path =
            format!("/admin/audit?action={}", String::from(js_sys::encode_uri_component(&filter.get_untracked())));
        if let Some(b) = before {
            path.push_str(&format!("&before={b}"));
        }
        spawn_local(async move {
            match api::get::<Vec<AuditEntry>>(&path).await {
                Ok(list) => {
                    more.set(list.len() == 100);
                    if append {
                        entries.update(|e| e.extend(list));
                    } else {
                        entries.set(list);
                    }
                }
                Err(e) => ctx.error(e),
            }
        });
    };
    load(false);
    let ws_name = move |id: Option<i64>| {
        id.and_then(|id| workspaces.with(|l| l.iter().find(|w| w.id == id).map(|w| w.name.clone()))).unwrap_or_default()
    };
    let export = move |_| {
        let mut csv = String::from("time,user,workspace,action,target,detail,ip\n");
        for e in entries.get_untracked() {
            csv.push_str(&format!(
                "{},{},{},{},{},{},{}\n",
                e.ts,
                csv_field(&e.username),
                e.workspace_id.map(|w| w.to_string()).unwrap_or_default(),
                csv_field(&e.action),
                csv_field(&e.target),
                csv_field(&e.detail),
                csv_field(&e.ip)
            ));
        }
        util::download("neko-audit-log.csv", "text/csv", &csv);
    };
    view! {
        <div class="audit-bar">
            <select prop:value=move || filter.get() on:change=move |ev| { filter.set(event_target_value(&ev)); load(false); }>
                {AUDIT_FILTERS.iter().map(|(v, l)| view! { <option value=*v>{*l}</option> }).collect_view()}
            </select>
            <button class="btn small ghost" on:click=move |_| load(false)><Icon svg=icons::REFRESH size=14 />"Refresh"</button>
            <button class="btn small ghost" on:click=export><Icon svg=icons::DOWNLOAD size=14 />"CSV"</button>
        </div>
        <div class="audit-list">
            <Show when=move || entries.with(|e| e.is_empty())><p class="muted">"Nothing recorded yet."</p></Show>
            <For each=move || entries.get() key=|e| e.id let:e>
                {
                    let area = e.action.split('.').next().unwrap_or("").to_string();
                    let when = util::ago(e.ts);
                    let full = String::from(js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(e.ts as f64 * 1000.0)).to_locale_string("en-GB", &wasm_bindgen::JsValue::UNDEFINED));
                    view! {
                        <div class="audit-row">
                            <span class="audit-time muted small" title=full>{when}</span>
                            <span class="audit-action" data-area=area>{e.action.clone()}</span>
                            <div class="audit-main">
                                <span>
                                    <strong>{if e.username.is_empty() { "—".to_string() } else { e.username.clone() }}</strong>
                                    {(!e.target.is_empty()).then(|| format!(" → {}", e.target))}
                                </span>
                                <span class="muted small">
                                    {e.detail.clone()}
                                    {move || { let w = ws_name(e.workspace_id); (!w.is_empty()).then(|| format!(" · {w}")) }}
                                    {(!e.ip.is_empty()).then(|| format!(" · {}", e.ip))}
                                </span>
                            </div>
                        </div>
                    }
                }
            </For>
        </div>
        <Show when=move || more.get() && entries.with(|e| !e.is_empty())>
            <button class="btn ghost wide" on:click=move |_| load(true)>"Load older"</button>
        </Show>
    }
}
