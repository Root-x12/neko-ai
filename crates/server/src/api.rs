//! JSON + SSE API under `/api`.

use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use neko_shared::*;
use serde::Deserialize;

use crate::{
    admin::{audit, audit_anon, ClientIp},
    auth::{self, AuthUser},
    chat, crypto, db, devices,
    error::{AppError, AppResult},
    llm::{self, Endpoint},
    uploads, AppState,
};

const MAX_LOGIN_FAILURES: u32 = 5;
const LOGIN_LOCKOUT_SECS: i64 = 60;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/auth/status", get(auth_status))
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
        .route("/auth/password", post(change_password))
        .route("/providers", get(list_providers).post(create_provider))
        .route("/providers/{id}", axum::routing::put(update_provider).delete(delete_provider))
        .route("/providers/{id}/models", get(provider_models))
        .route("/conversations", get(list_conversations).post(create_conversation))
        .route("/conversations/{id}", get(get_conversation).patch(patch_conversation).delete(delete_conversation))
        .route("/conversations/{id}/messages", post(chat::send_message))
        .route("/conversations/{id}/regenerate", post(chat::regenerate))
        .route("/uploads", post(uploads::upload).layer(DefaultBodyLimit::max(uploads::MAX_UPLOAD + 64 * 1024)))
        .route("/uploads/{id}", get(uploads::download).delete(uploads::remove))
        .route("/tools/search", get(get_search_settings).put(put_search_settings))
        .route("/mcp", get(list_mcp).post(create_mcp))
        .route("/mcp/{id}", put(update_mcp).delete(delete_mcp))
        .route("/mcp/{id}/test", post(test_mcp))
        .route("/search", get(search))
        .route("/devices", get(devices_info))
        .route("/devices/{id}", delete(revoke_device))
        .route("/devices/revoke-others", post(revoke_other_devices))
        .merge(crate::admin::routes())
        .fallback(|| async { AppError::not_found("API route") })
}

fn no_content() -> StatusCode {
    StatusCode::NO_CONTENT
}

// ---------- auth ----------

async fn auth_status(State(s): State<AppState>) -> AppResult<Json<AuthStatus>> {
    let count = s.db.call(|c| db::user_count(c)).await?;
    Ok(Json(AuthStatus { needs_setup: count == 0, signup_open: count == 0 || s.config.allow_signup }))
}

pub(crate) fn validate_credentials(username: &str, password: &str) -> AppResult<()> {
    let ok_name = (3..=32).contains(&username.chars().count())
        && username.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if !ok_name {
        return Err(AppError::bad_request("Username must be 3–32 characters: letters, numbers, _ - ."));
    }
    validate_password(password)
}

pub(crate) fn validate_password(password: &str) -> AppResult<()> {
    if !(8..=256).contains(&password.chars().count()) {
        return Err(AppError::bad_request("Password must be at least 8 characters"));
    }
    Ok(())
}

fn user_agent(headers: &HeaderMap) -> String {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("Unknown device")
        .chars()
        .take(300)
        .collect()
}

async fn start_session(s: &AppState, user: User, headers: &HeaderMap) -> AppResult<Response> {
    let token = crypto::random_token();
    let hash = crypto::sha256_hex(&token);
    let ua = user_agent(headers);
    let uid = user.id;
    s.db.call(move |c| {
        db::purge_expired_sessions(c)?;
        db::create_session(c, &hash, uid, &ua, auth::SESSION_TTL_SECS)
    })
    .await?;
    let cookie = auth::session_cookie(&token, s.config.secure_cookies);
    Ok(([(header::SET_COOKIE, cookie)], Json(user)).into_response())
}

async fn register(
    State(s): State<AppState>,
    headers: HeaderMap,
    ip: ClientIp,
    Json(req): Json<RegisterRequest>,
) -> AppResult<Response> {
    let username = req.username.trim().to_string();
    validate_credentials(&username, &req.password)?;
    let count = s.db.call(|c| db::user_count(c)).await?;
    let role = if count == 0 {
        let expected = s.setup_code.lock().unwrap().clone();
        let given = req.setup_code.as_deref().unwrap_or("").trim().to_ascii_uppercase();
        match expected {
            Some(code) if crypto::ct_eq(&code, &given) => Role::Admin,
            Some(_) => return Err(AppError::forbidden("Wrong setup code. It is printed in the server console.")),
            None => Role::Admin,
        }
    } else if s.config.allow_signup {
        Role::User
    } else {
        return Err(AppError::forbidden("Sign-up is closed. Ask the admin for an account."));
    };
    let hash = crypto::hash_password(&req.password).map_err(AppError::internal)?;
    let user =
        s.db.call(move |c| {
            // Re-check inside the lock so two racing first sign-ups can't both become admin.
            if role == Role::Admin && db::user_count(c)? > 0 {
                return Ok(None);
            }
            match db::create_user(c, &username, &hash, role, 1) {
                Ok(u) => Ok(Some(u)),
                Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::ConstraintViolation => {
                    Ok(None)
                }
                Err(e) => Err(e),
            }
        })
        .await?
        .ok_or_else(|| AppError::new(StatusCode::CONFLICT, "That username is taken"))?;
    if role == Role::Admin {
        *s.setup_code.lock().unwrap() = None;
    }
    tracing::info!(user = %user.username, role = user.role.as_str(), "account created");
    audit(&s, Some(&user), &ip.0, "auth.register", &user.username, user.role.as_str()).await;
    start_session(&s, user, &headers).await
}

async fn login(
    State(s): State<AppState>,
    headers: HeaderMap,
    ip: ClientIp,
    Json(req): Json<LoginRequest>,
) -> AppResult<Response> {
    let key = req.username.trim().to_lowercase();
    {
        let failures = s.login_failures.lock().unwrap();
        if let Some(&(n, last)) = failures.get(&key) {
            if n >= MAX_LOGIN_FAILURES && db::now() - last < LOGIN_LOCKOUT_SECS {
                return Err(AppError::new(StatusCode::TOO_MANY_REQUESTS, "Too many attempts. Try again in a minute."));
            }
        }
    }
    let name = req.username.trim().to_string();
    let record = s.db.call(move |c| db::find_user_by_name(c, &name)).await?;
    let password = req.password;
    let verified =
        tokio::task::spawn_blocking(move || record.filter(|r| crypto::verify_password(&password, &r.password_hash)))
            .await
            .map_err(AppError::internal)?;
    match verified {
        Some(rec) if rec.disabled => {
            audit(&s, Some(&rec.user), &ip.0, "auth.login_disabled", &rec.user.username, "").await;
            Err(AppError::forbidden("This account has been disabled. Ask the admin."))
        }
        Some(rec) => {
            s.login_failures.lock().unwrap().remove(&key);
            audit(&s, Some(&rec.user), &ip.0, "auth.login", &rec.user.username, user_agent(&headers)).await;
            start_session(&s, rec.user, &headers).await
        }
        None => {
            audit_anon(&s, req.username.trim(), &ip.0, "auth.login_failed", "").await;
            let mut failures = s.login_failures.lock().unwrap();
            let now = db::now();
            let entry = failures.entry(key).or_insert((0, now));
            if now - entry.1 >= LOGIN_LOCKOUT_SECS {
                entry.0 = 0;
            }
            entry.0 += 1;
            entry.1 = now;
            drop(failures);
            Err(AppError::new(StatusCode::UNAUTHORIZED, "Wrong username or password"))
        }
    }
}

async fn logout(State(s): State<AppState>, headers: HeaderMap) -> AppResult<Response> {
    if let Some(token) = auth::session_token(&headers) {
        let hash = crypto::sha256_hex(&token);
        s.db.call(move |c| db::delete_session(c, &hash)).await?;
    }
    Ok(([(header::SET_COOKIE, auth::clear_cookie())], StatusCode::NO_CONTENT).into_response())
}

async fn me(a: AuthUser) -> Json<User> {
    Json(a.user)
}

async fn change_password(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    Json(req): Json<ChangePasswordRequest>,
) -> AppResult<StatusCode> {
    validate_password(&req.new_password)?;
    let uid = a.user.id;
    let current_hash = s.db.call(move |c| db::password_hash(c, uid)).await?;
    let new_hash = tokio::task::spawn_blocking(move || {
        if crypto::verify_password(&req.current_password, &current_hash) {
            crypto::hash_password(&req.new_password).map(Some)
        } else {
            Ok(None)
        }
    })
    .await
    .map_err(AppError::internal)?
    .map_err(AppError::internal)?
    .ok_or_else(|| AppError::forbidden("Current password is wrong"))?;
    let sid = a.session_id;
    s.db.call(move |c| {
        db::set_password_hash(c, uid, &new_hash)?;
        db::delete_other_sessions(c, uid, sid)
    })
    .await?;
    audit(&s, Some(&a.user), &ip.0, "auth.password_change", &a.user.username, "").await;
    Ok(no_content())
}

// ---------- providers ----------

async fn list_providers(State(s): State<AppState>, a: AuthUser) -> AppResult<Json<Vec<Provider>>> {
    Ok(Json(s.db.call(move |c| db::list_providers(c, a.user.id)).await?))
}

fn clean_provider_input(input: &ProviderInput) -> AppResult<(String, String)> {
    let name = input.name.trim().to_string();
    if name.is_empty() || name.chars().count() > 60 {
        return Err(AppError::bad_request("Give the provider a name (up to 60 characters)"));
    }
    let base_url = input.base_url.trim().trim_end_matches('/').to_string();
    if !(base_url.starts_with("http://") || base_url.starts_with("https://")) || base_url.len() > 500 {
        return Err(AppError::bad_request("Base URL must start with http:// or https://"));
    }
    Ok((name, base_url))
}

fn encrypt_key(s: &AppState, key: &str) -> Option<(Vec<u8>, String)> {
    let key = key.trim();
    (!key.is_empty()).then(|| (s.vault.encrypt(key), crypto::key_hint(key)))
}

/// In local-only mode, every provider and MCP server must live on this machine or the LAN.
pub(crate) async fn ensure_local(s: &AppState, url: &str) -> AppResult<()> {
    if s.config.local_only {
        crate::tools::web::ensure_local_url(url).await.map_err(AppError::bad_request)?;
    }
    Ok(())
}

async fn create_provider(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    Json(input): Json<ProviderInput>,
) -> AppResult<Json<Provider>> {
    let (name, base_url) = clean_provider_input(&input)?;
    ensure_local(&s, &base_url).await?;
    let key = input.api_key.as_deref().and_then(|k| encrypt_key(&s, k));
    let uid = a.user.id;
    let provider =
        s.db.call(move |c| {
            let id = db::insert_provider(c, uid, &name, input.kind, &base_url, key)?;
            db::get_provider(c, uid, id)
        })
        .await?
        .ok_or_else(|| AppError::internal("provider vanished after insert"))?;
    let p = &provider.provider;
    audit(&s, Some(&a.user), &ip.0, "provider.create", &p.name, &p.base_url).await;
    Ok(Json(provider.provider))
}

async fn update_provider(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    Path(id): Path<i64>,
    Json(input): Json<ProviderInput>,
) -> AppResult<Json<Provider>> {
    let (name, base_url) = clean_provider_input(&input)?;
    ensure_local(&s, &base_url).await?;
    let key_change = match input.api_key.as_deref() {
        None => "",
        Some("") => " (key removed)",
        Some(_) => " (key replaced)",
    };
    let key = input.api_key.as_deref().map(|k| encrypt_key(&s, k));
    let uid = a.user.id;
    let provider =
        s.db.call(move |c| {
            db::update_provider(c, uid, id, &name, input.kind, &base_url, key)?;
            db::get_provider(c, uid, id)
        })
        .await?
        .ok_or_else(|| AppError::not_found("Provider"))?;
    let p = &provider.provider;
    audit(&s, Some(&a.user), &ip.0, "provider.update", &p.name, format!("{}{key_change}", p.base_url)).await;
    Ok(Json(provider.provider))
}

async fn delete_provider(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let uid = a.user.id;
    if s.db.call(move |c| db::delete_provider(c, uid, id)).await? {
        audit(&s, Some(&a.user), &ip.0, "provider.delete", id.to_string(), "").await;
        Ok(no_content())
    } else {
        Err(AppError::not_found("Provider"))
    }
}

pub(crate) async fn endpoint_for(s: &AppState, user_id: i64, provider_id: i64) -> AppResult<Endpoint> {
    let rec =
        s.db.call(move |c| db::get_provider(c, user_id, provider_id))
            .await?
            .ok_or_else(|| AppError::not_found("Provider"))?;
    ensure_local(s, &rec.provider.base_url).await?;
    let api_key = match rec.api_key_enc {
        Some(blob) => Some(s.vault.decrypt(&blob).map_err(|e| AppError::bad_request(e.to_string()))?),
        None => None,
    };
    Ok(Endpoint { kind: rec.provider.kind, base_url: rec.provider.base_url, api_key })
}

async fn provider_models(State(s): State<AppState>, a: AuthUser, Path(id): Path<i64>) -> AppResult<Json<ModelList>> {
    let ep = endpoint_for(&s, a.user.id, id).await?;
    let mut models = llm::list_models(&s.http, &ep).await.map_err(AppError::bad_gateway)?;
    let allow = crate::admin::workspace_of(&s, &a.user).await.model_allowlist;
    models.retain(|m| crate::guard::model_allowed(&allow, m));
    Ok(Json(ModelList { models }))
}

// ---------- conversations ----------

async fn list_conversations(State(s): State<AppState>, a: AuthUser) -> AppResult<Json<Vec<ConversationSummary>>> {
    Ok(Json(s.db.call(move |c| db::list_conversations(c, a.user.id)).await?))
}

#[derive(Deserialize, Default)]
struct NewConversation {
    #[serde(default)]
    title: Option<String>,
}

pub(crate) const DEFAULT_TITLE: &str = "New chat";

async fn create_conversation(
    State(s): State<AppState>,
    a: AuthUser,
    body: Option<Json<NewConversation>>,
) -> AppResult<Json<Conversation>> {
    let title = body
        .and_then(|b| b.0.title)
        .map(|t| clean_title(&t))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| DEFAULT_TITLE.to_string());
    let uid = a.user.id;
    let conv =
        s.db.call(move |c| {
            let id = db::create_conversation(c, uid, &title)?;
            db::get_conversation(c, uid, id)
        })
        .await?
        .ok_or_else(|| AppError::internal("conversation vanished after insert"))?;
    Ok(Json(conv))
}

async fn get_conversation(
    State(s): State<AppState>,
    a: AuthUser,
    Path(id): Path<i64>,
) -> AppResult<Json<Conversation>> {
    s.db.call(move |c| db::get_conversation(c, a.user.id, id))
        .await?
        .map(Json)
        .ok_or_else(|| AppError::not_found("Conversation"))
}

pub(crate) fn clean_title(t: &str) -> String {
    t.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(120).collect()
}

pub(crate) fn validate_params(p: &ChatParams) -> AppResult<()> {
    if p.temperature.is_some_and(|t| !(0.0..=2.0).contains(&t)) {
        return Err(AppError::bad_request("Temperature must be between 0 and 2"));
    }
    if p.top_p.is_some_and(|t| !(0.0..=1.0).contains(&t)) {
        return Err(AppError::bad_request("Top-p must be between 0 and 1"));
    }
    if p.max_tokens.is_some_and(|m| m == 0 || m > 1_000_000) {
        return Err(AppError::bad_request("Max tokens must be between 1 and 1,000,000"));
    }
    if p.system_prompt.chars().count() > 50_000 {
        return Err(AppError::bad_request("System prompt is too long (50,000 characters max)"));
    }
    Ok(())
}

async fn patch_conversation(
    State(s): State<AppState>,
    a: AuthUser,
    Path(id): Path<i64>,
    Json(patch): Json<ConversationPatch>,
) -> AppResult<Json<Conversation>> {
    if let Some(p) = &patch.params {
        validate_params(p)?;
    }
    let title = patch.title.as_deref().map(clean_title);
    if title.as_deref() == Some("") {
        return Err(AppError::bad_request("Title can't be empty"));
    }
    let uid = a.user.id;
    if let Some(pid) = patch.provider_id {
        // Make sure the provider belongs to this user.
        s.db.call(move |c| db::get_provider(c, uid, pid)).await?.ok_or_else(|| AppError::not_found("Provider"))?;
    }
    s.db.call(move |c| {
        if db::get_conversation(c, uid, id)?.is_none() {
            return Ok(None);
        }
        if let Some(t) = &title {
            db::rename_conversation(c, uid, id, t)?;
        }
        if let (Some(pid), Some(model)) = (patch.provider_id, &patch.model) {
            db::set_conversation_model(c, uid, id, pid, model.trim())?;
        }
        if let Some(p) = &patch.params {
            db::set_conversation_params(c, uid, id, p)?;
        }
        db::get_conversation(c, uid, id)
    })
    .await?
    .map(Json)
    .ok_or_else(|| AppError::not_found("Conversation"))
}

async fn delete_conversation(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let uid = a.user.id;
    match s.db.call(move |c| db::delete_conversation(c, uid, id)).await? {
        Some(paths) => {
            uploads::remove_files(&s.config.data_dir, paths).await;
            audit(&s, Some(&a.user), &ip.0, "chat.delete", id.to_string(), "").await;
            Ok(no_content())
        }
        None => Err(AppError::not_found("Conversation")),
    }
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

async fn search(
    State(s): State<AppState>,
    a: AuthUser,
    Query(q): Query<SearchQuery>,
) -> AppResult<Json<Vec<SearchHit>>> {
    let q = q.q.trim().chars().take(200).collect::<String>();
    if q.is_empty() {
        return Ok(Json(Vec::new()));
    }
    Ok(Json(s.db.call(move |c| db::search(c, a.user.id, &q)).await?))
}

// ---------- devices ----------

async fn devices_info(State(s): State<AppState>, a: AuthUser) -> AppResult<Json<DevicesInfo>> {
    let urls = devices::reachable_urls(s.config.bind);
    // A QR code only helps when another device can actually reach the address.
    let qr_svg = urls.first().filter(|u| !u.contains("localhost")).and_then(|u| devices::qr_svg(u));
    let sessions = s.db.call(move |c| db::list_sessions(c, a.user.id, a.session_id)).await?;
    Ok(Json(DevicesInfo { urls, qr_svg, sessions }))
}

async fn revoke_device(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    if id == a.session_id {
        return Err(AppError::bad_request("Use “Sign out” to end this device's session"));
    }
    let uid = a.user.id;
    if s.db.call(move |c| db::delete_session_by_id(c, uid, id)).await? {
        audit(&s, Some(&a.user), &ip.0, "device.revoke", id.to_string(), "").await;
        Ok(no_content())
    } else {
        Err(AppError::not_found("Device"))
    }
}

async fn revoke_other_devices(State(s): State<AppState>, a: AuthUser, ip: ClientIp) -> AppResult<StatusCode> {
    let (uid, sid) = (a.user.id, a.session_id);
    s.db.call(move |c| db::delete_other_sessions(c, uid, sid)).await?;
    audit(&s, Some(&a.user), &ip.0, "device.revoke_others", "", "").await;
    Ok(no_content())
}

// ---------- tools: web search settings ----------

async fn get_search_settings(State(s): State<AppState>, a: AuthUser) -> AppResult<Json<SearchSettings>> {
    Ok(Json(s.db.call(move |c| db::search_settings_record(c, a.user.id)).await?.settings))
}

async fn put_search_settings(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    Json(input): Json<SearchSettingsInput>,
) -> AppResult<Json<SearchSettings>> {
    let base_url = input.base_url.trim().trim_end_matches('/').to_string();
    if input.backend == SearchBackend::Searxng && !(base_url.starts_with("http://") || base_url.starts_with("https://"))
    {
        return Err(AppError::bad_request("SearXNG URL must start with http:// or https://"));
    }
    let key = input.api_key.as_deref().map(|k| encrypt_key(&s, k));
    let uid = a.user.id;
    let settings =
        s.db.call(move |c| {
            db::save_search_settings(c, uid, input.backend, &base_url, key)?;
            db::search_settings_record(c, uid)
        })
        .await?
        .settings;
    audit(&s, Some(&a.user), &ip.0, "tools.search_settings", settings.backend.as_str(), "").await;
    Ok(Json(settings))
}

// ---------- tools: MCP servers ----------

async fn list_mcp(State(s): State<AppState>, a: AuthUser) -> AppResult<Json<Vec<McpServer>>> {
    Ok(Json(s.db.call(move |c| db::list_mcp_servers(c, a.user.id)).await?))
}

fn seal_pairs(
    s: &AppState,
    pairs: Option<Vec<(String, String)>>,
    header_names: bool,
) -> AppResult<Option<db::SecretPairs>> {
    let Some(pairs) = pairs else { return Ok(None) };
    let pairs: Vec<(String, String)> = pairs
        .into_iter()
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .filter(|(k, _)| !k.is_empty())
        .collect();
    for (k, v) in &pairs {
        let ok = if header_names {
            HeaderName::from_bytes(k.as_bytes()).is_ok() && HeaderValue::from_str(v).is_ok()
        } else {
            k.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        };
        if !ok {
            let what = if header_names { "header" } else { "environment variable" };
            return Err(AppError::bad_request(format!("“{k}” is not a valid {what}")));
        }
    }
    let names = pairs.iter().map(|(k, _)| k.clone()).collect();
    let json = serde_json::to_string(&pairs).map_err(AppError::internal)?;
    Ok(Some((s.vault.encrypt(&json), names)))
}

async fn save_mcp(
    s: AppState,
    a: AuthUser,
    ip: ClientIp,
    id: Option<i64>,
    input: McpServerInput,
) -> AppResult<Json<McpServer>> {
    let name = input.name.trim().to_string();
    if name.is_empty() || name.chars().count() > 60 {
        return Err(AppError::bad_request("Give the server a name (up to 60 characters)"));
    }
    let url = input.url.trim().to_string();
    let command = input.command.trim().to_string();
    match input.transport {
        McpTransport::Http => {
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(AppError::bad_request("Server URL must start with http:// or https://"));
            }
            ensure_local(&s, &url).await?;
        }
        McpTransport::Stdio => {
            if a.user.role != Role::Admin {
                return Err(AppError::forbidden(
                    "Only the admin can add local-command MCP servers (they run on the server)",
                ));
            }
            if command.is_empty() {
                return Err(AppError::bad_request("Enter the command to run, e.g. npx"));
            }
        }
    }
    let args: Vec<String> = input.args.iter().map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
    let headers = seal_pairs(&s, input.headers, true)?;
    let env = seal_pairs(&s, input.env, false)?;
    let uid = a.user.id;
    let saved =
        s.db.call(move |c| {
            let Some(id) = db::save_mcp_server(
                c,
                uid,
                id,
                &name,
                input.transport,
                &url,
                &command,
                &args,
                headers,
                env,
                input.enabled,
            )?
            else {
                return Ok(None);
            };
            db::get_mcp_record(c, uid, id)
        })
        .await?
        .ok_or_else(|| AppError::not_found("MCP server"))?;
    s.mcp.forget(saved.server.id).await;
    let m = &saved.server;
    let target =
        if m.transport == McpTransport::Http { m.url.clone() } else { format!("{} {}", m.command, m.args.join(" ")) };
    let action = if id.is_some() { "mcp.update" } else { "mcp.create" };
    audit(&s, Some(&a.user), &ip.0, action, &m.name, target).await;
    Ok(Json(saved.server))
}

async fn create_mcp(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    Json(input): Json<McpServerInput>,
) -> AppResult<Json<McpServer>> {
    save_mcp(s, a, ip, None, input).await
}

async fn update_mcp(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    Path(id): Path<i64>,
    Json(input): Json<McpServerInput>,
) -> AppResult<Json<McpServer>> {
    save_mcp(s, a, ip, Some(id), input).await
}

async fn delete_mcp(
    State(s): State<AppState>,
    a: AuthUser,
    ip: ClientIp,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let uid = a.user.id;
    if s.db.call(move |c| db::delete_mcp_server(c, uid, id)).await? {
        s.mcp.forget(id).await;
        audit(&s, Some(&a.user), &ip.0, "mcp.delete", id.to_string(), "").await;
        Ok(no_content())
    } else {
        Err(AppError::not_found("MCP server"))
    }
}

async fn test_mcp(State(s): State<AppState>, a: AuthUser, Path(id): Path<i64>) -> AppResult<Json<McpTestResult>> {
    let uid = a.user.id;
    let rec =
        s.db.call(move |c| db::get_mcp_record(c, uid, id)).await?.ok_or_else(|| AppError::not_found("MCP server"))?;
    if rec.server.transport == McpTransport::Stdio && a.user.role != Role::Admin {
        return Err(AppError::forbidden("Local-command MCP servers are admin-only"));
    }
    let cfg = rec.decrypt(&s.vault).map_err(AppError::bad_request)?;
    s.mcp.forget(id).await;
    let tools = s.mcp.list_tools(&s.http, &cfg).await.map_err(AppError::bad_gateway)?;
    Ok(Json(McpTestResult {
        server_name: cfg.name,
        tools: tools.into_iter().map(|t| McpToolInfo { name: t.name, description: t.description }).collect(),
    }))
}
