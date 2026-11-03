//! Governance: audit log, roles, workspaces (model allowlists, retention, guard policies),
//! user administration and periodic maintenance.

use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, FromRequestParts, Path, Query, State},
    http::{request::Parts, StatusCode},
    routing::{get, patch},
    Json, Router,
};
use neko_shared::*;
use serde::Deserialize;

use crate::{
    auth::AuthUser,
    crypto, db,
    error::{AppError, AppResult},
    uploads, AppState,
};

/// The caller's address: the TCP peer, or `X-Forwarded-For` when `NEKO_TRUST_PROXY` is set.
pub struct ClientIp(pub String);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        if state.config.trust_proxy {
            if let Some(fwd) = parts.headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
                if let Some(first) = fwd.split(',').next().map(str::trim).filter(|s| !s.is_empty()) {
                    return Ok(ClientIp(first.chars().take(64).collect()));
                }
            }
        }
        let ip = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip().to_string()).unwrap_or_default();
        Ok(ClientIp(ip))
    }
}

/// Records an audit event. Never fails the request: a logging problem is only reported in the log.
pub async fn audit(
    s: &AppState,
    user: Option<&User>,
    ip: &str,
    action: &str,
    target: impl Into<String>,
    detail: impl Into<String>,
) {
    let event = db::AuditEvent {
        user_id: user.map(|u| u.id),
        username: user.map(|u| u.username.clone()).unwrap_or_default(),
        workspace_id: user.map(|u| u.workspace_id),
        action: action.to_string(),
        target: target.into(),
        detail: detail.into(),
        ip: ip.to_string(),
    };
    if let Err(e) = s.db.call(move |c| db::insert_audit(c, &event)).await {
        tracing::error!("writing audit log: {e}");
    }
}

/// Same as [`audit`] for events about a user who isn't signed in (e.g. failed logins).
pub async fn audit_anon(s: &AppState, username: &str, ip: &str, action: &str, detail: &str) {
    let event = db::AuditEvent {
        username: username.chars().take(64).collect(),
        action: action.into(),
        detail: detail.into(),
        ip: ip.into(),
        ..Default::default()
    };
    let _ = s.db.call(move |c| db::insert_audit(c, &event)).await;
}

/// The policies that apply to `user`, falling back to safe defaults.
pub async fn workspace_of(s: &AppState, user: &User) -> Workspace {
    let id = user.workspace_id;
    s.db.call(move |c| db::get_workspace(c, id)).await.ok().flatten().unwrap_or(Workspace {
        id,
        name: "Default".into(),
        model_allowlist: Vec::new(),
        retention_days: None,
        secret_policy: GuardPolicy::Warn,
        pii_policy: GuardPolicy::Warn,
        injection_scan: true,
        member_count: 0,
    })
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/info", get(info))
        .route("/admin/users", get(list_users).post(create_user))
        .route("/admin/users/{id}", patch(update_user).delete(delete_user))
        .route("/admin/workspaces", get(list_workspaces).post(create_workspace))
        .route("/admin/workspaces/{id}", axum::routing::put(update_workspace).delete(delete_workspace))
        .route("/admin/audit", get(list_audit))
}

fn require_admin(a: &AuthUser) -> AppResult<()> {
    if a.user.role == Role::Admin {
        Ok(())
    } else {
        Err(AppError::forbidden("Only the admin can do that"))
    }
}

/// Admins manage every workspace; team leads only their own.
fn require_workspace_manager(a: &AuthUser, workspace_id: i64) -> AppResult<()> {
    match a.user.role {
        Role::Admin => Ok(()),
        Role::Team if a.user.workspace_id == workspace_id => Ok(()),
        _ => Err(AppError::forbidden("You can't manage this workspace")),
    }
}

async fn info(State(s): State<AppState>, a: AuthUser) -> Json<ServerInfo> {
    Json(ServerInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        local_only: s.config.local_only,
        workspace: workspace_of(&s, &a.user).await,
    })
}

// ---------- users ----------

async fn list_users(State(s): State<AppState>, a: AuthUser) -> AppResult<Json<Vec<AdminUser>>> {
    require_admin(&a)?;
    Ok(Json(s.db.call(|c| db::list_users(c)).await?))
}

async fn workspace_exists(s: &AppState, id: i64) -> AppResult<()> {
    s.db.call(move |c| db::get_workspace(c, id)).await?.map(|_| ()).ok_or_else(|| AppError::not_found("Workspace"))
}

async fn create_user(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    Json(req): Json<CreateUser>,
) -> AppResult<Json<AdminUser>> {
    require_admin(&a)?;
    let username = req.username.trim().to_string();
    crate::api::validate_credentials(&username, &req.password)?;
    workspace_exists(&s, req.workspace_id).await?;
    let hash = crypto::hash_password(&req.password).map_err(AppError::internal)?;
    let (role, ws, name) = (req.role, req.workspace_id, username.clone());
    let created =
        s.db.call(move |c| match db::create_user(c, &name, &hash, role, ws) {
            Ok(u) => db::get_admin_user(c, u.id),
            Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::ConstraintViolation => Ok(None),
            Err(e) => Err(e),
        })
        .await?
        .ok_or_else(|| AppError::new(StatusCode::CONFLICT, "That username is taken"))?;
    audit(&s, Some(&a.user), &ip.0, "admin.user_create", &username, format!("role={} workspace={ws}", role.as_str()))
        .await;
    Ok(Json(created))
}

async fn update_user(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    Path(id): Path<i64>,
    Json(req): Json<UpdateUser>,
) -> AppResult<Json<AdminUser>> {
    require_admin(&a)?;
    let target = s.db.call(move |c| db::get_admin_user(c, id)).await?.ok_or_else(|| AppError::not_found("User"))?;
    let demoting =
        target.role == Role::Admin && (req.role.is_some_and(|r| r != Role::Admin) || req.disabled == Some(true));
    if demoting && s.db.call(|c| db::active_admin_count(c)).await? <= 1 {
        return Err(AppError::bad_request("There must always be at least one active admin"));
    }
    if let Some(ws) = req.workspace_id {
        workspace_exists(&s, ws).await?;
    }
    let hash = match req.password.as_deref() {
        Some(p) => {
            crate::api::validate_password(p)?;
            Some(crypto::hash_password(p).map_err(AppError::internal)?)
        }
        None => None,
    };
    let mut changes = Vec::new();
    if let Some(r) = req.role {
        changes.push(format!("role={}", r.as_str()));
    }
    if let Some(w) = req.workspace_id {
        changes.push(format!("workspace={w}"));
    }
    if let Some(d) = req.disabled {
        changes.push(if d { "disabled".into() } else { "enabled".into() });
    }
    if hash.is_some() {
        changes.push("password reset".into());
    }
    let updated =
        s.db.call(move |c| {
            db::update_user(c, id, &req, hash.as_deref())?;
            db::get_admin_user(c, id)
        })
        .await?
        .ok_or_else(|| AppError::not_found("User"))?;
    audit(&s, Some(&a.user), &ip.0, "admin.user_update", &target.username, changes.join(", ")).await;
    Ok(Json(updated))
}

async fn delete_user(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    require_admin(&a)?;
    if id == a.user.id {
        return Err(AppError::bad_request("You can't delete your own account here"));
    }
    let target = s.db.call(move |c| db::get_admin_user(c, id)).await?.ok_or_else(|| AppError::not_found("User"))?;
    if target.role == Role::Admin && !target.disabled && s.db.call(|c| db::active_admin_count(c)).await? <= 1 {
        return Err(AppError::bad_request("There must always be at least one active admin"));
    }
    let paths = s.db.call(move |c| db::delete_user(c, id)).await?.ok_or_else(|| AppError::not_found("User"))?;
    uploads::remove_files(&s.config.data_dir, paths).await;
    audit(&s, Some(&a.user), &ip.0, "admin.user_delete", &target.username, "").await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- workspaces ----------

async fn list_workspaces(State(s): State<AppState>, a: AuthUser) -> AppResult<Json<Vec<Workspace>>> {
    let all = s.db.call(|c| db::list_workspaces(c)).await?;
    Ok(Json(match a.user.role {
        Role::Admin => all,
        Role::Team => all.into_iter().filter(|w| w.id == a.user.workspace_id).collect(),
        Role::User => return Err(AppError::forbidden("Only admins and team leads manage workspaces")),
    }))
}

fn clean_workspace(mut w: WorkspaceInput) -> AppResult<WorkspaceInput> {
    w.name = w.name.split_whitespace().collect::<Vec<_>>().join(" ");
    if w.name.is_empty() || w.name.chars().count() > 60 {
        return Err(AppError::bad_request("Give the workspace a name (up to 60 characters)"));
    }
    w.model_allowlist =
        w.model_allowlist.iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).take(200).collect();
    if w.model_allowlist.iter().any(|p| p.len() > 120) {
        return Err(AppError::bad_request("Model patterns can be at most 120 characters"));
    }
    if w.retention_days == Some(0) || w.retention_days.is_some_and(|d| d > 36_500) {
        return Err(AppError::bad_request("Retention must be between 1 and 36,500 days (or off)"));
    }
    Ok(w)
}

fn describe(w: &WorkspaceInput) -> String {
    format!(
        "secrets={} pii={} injection_scan={} retention={} allowlist=[{}]",
        w.secret_policy.as_str(),
        w.pii_policy.as_str(),
        w.injection_scan,
        w.retention_days.map(|d| format!("{d}d")).unwrap_or_else(|| "off".into()),
        w.model_allowlist.join(", ")
    )
}

async fn save_ws(s: &AppState, id: Option<i64>, input: WorkspaceInput) -> AppResult<Workspace> {
    s.db.call(move |c| match db::save_workspace(c, id, &input) {
        Ok(Some(id)) => db::get_workspace(c, id).map(|w| w.map(Ok)),
        Ok(None) => Ok(None),
        Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::ConstraintViolation => {
            Ok(Some(Err(())))
        }
        Err(e) => Err(e),
    })
    .await?
    .ok_or_else(|| AppError::not_found("Workspace"))?
    .map_err(|_| AppError::new(StatusCode::CONFLICT, "A workspace with that name exists"))
}

async fn create_workspace(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    Json(input): Json<WorkspaceInput>,
) -> AppResult<Json<Workspace>> {
    require_admin(&a)?;
    let input = clean_workspace(input)?;
    let detail = describe(&input);
    let w = save_ws(&s, None, input).await?;
    audit(&s, Some(&a.user), &ip.0, "admin.workspace_create", &w.name, detail).await;
    Ok(Json(w))
}

async fn update_workspace(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    Path(id): Path<i64>,
    Json(input): Json<WorkspaceInput>,
) -> AppResult<Json<Workspace>> {
    require_workspace_manager(&a, id)?;
    let input = clean_workspace(input)?;
    let detail = describe(&input);
    let w = save_ws(&s, Some(id), input).await?;
    audit(&s, Some(&a.user), &ip.0, "admin.workspace_update", &w.name, detail).await;
    Ok(Json(w))
}

async fn delete_workspace(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    require_admin(&a)?;
    let w = s.db.call(move |c| db::get_workspace(c, id)).await?.ok_or_else(|| AppError::not_found("Workspace"))?;
    if !s.db.call(move |c| db::delete_workspace(c, id)).await? {
        return Err(AppError::bad_request("Only empty workspaces can be deleted (and never the default one)"));
    }
    audit(&s, Some(&a.user), &ip.0, "admin.workspace_delete", &w.name, "").await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- audit ----------

#[derive(Deserialize)]
struct AuditQuery {
    #[serde(default)]
    before: Option<i64>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    workspace: Option<i64>,
}

async fn list_audit(
    State(s): State<AppState>,
    a: AuthUser,
    Query(q): Query<AuditQuery>,
) -> AppResult<Json<Vec<AuditEntry>>> {
    let workspace = match a.user.role {
        Role::Admin => q.workspace,
        Role::Team => Some(a.user.workspace_id),
        Role::User => return Err(AppError::forbidden("Only admins and team leads can read the audit log")),
    };
    let action = q.action.filter(|x| !x.trim().is_empty()).map(|x| x.trim().chars().take(40).collect::<String>());
    Ok(Json(s.db.call(move |c| db::list_audit(c, workspace, action.as_deref(), q.before, 100)).await?))
}

// ---------- maintenance ----------

/// Hourly housekeeping: unsent uploads, workspace retention, old audit entries.
pub async fn maintenance(s: &AppState) {
    uploads::sweep(s).await;
    let now = db::now();
    match s.db.call(move |c| db::retention_sweep(c, now)).await {
        Ok((n, paths)) => {
            uploads::remove_files(&s.config.data_dir, paths).await;
            if n > 0 {
                tracing::info!("retention: deleted {n} expired chats");
                audit_anon(s, "system", "", "retention.delete", &format!("{n} chats")).await;
            }
        }
        Err(e) => tracing::error!("retention sweep: {e}"),
    }
    if let Some(days) = s.config.audit_retention_days {
        let cutoff = now - i64::from(days) * 86_400;
        if let Err(e) = s.db.call(move |c| db::purge_audit(c, cutoff)).await {
            tracing::error!("audit purge: {e}");
        }
    }
}
