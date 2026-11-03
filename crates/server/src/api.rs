//! JSON + SSE API under `/api`.

use std::{convert::Infallible, time::Duration};

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{delete, get, post},
    Json, Router,
};
use futures_util::{Stream, StreamExt};
use neko_shared::*;
use serde::Deserialize;

use crate::{
    auth::{self, AuthUser},
    crypto, db, devices,
    error::{AppError, AppResult},
    llm::{self, ChatTurn, Endpoint},
    AppState,
};

const MAX_MESSAGE_CHARS: usize = 200_000;
/// Error marker for answers the user stopped; their partial text is still sent as context.
const STOPPED: &str = "Stopped";
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
        .route("/conversations/{id}/messages", post(send_message))
        .route("/conversations/{id}/regenerate", post(regenerate))
        .route("/search", get(search))
        .route("/devices", get(devices_info))
        .route("/devices/{id}", delete(revoke_device))
        .route("/devices/revoke-others", post(revoke_other_devices))
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

fn validate_credentials(username: &str, password: &str) -> AppResult<()> {
    let ok_name = (3..=32).contains(&username.chars().count())
        && username.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if !ok_name {
        return Err(AppError::bad_request("Username must be 3–32 characters: letters, numbers, _ - ."));
    }
    validate_password(password)
}

fn validate_password(password: &str) -> AppResult<()> {
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
            match db::create_user(c, &username, &hash, role) {
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
    start_session(&s, user, &headers).await
}

async fn login(State(s): State<AppState>, headers: HeaderMap, Json(req): Json<LoginRequest>) -> AppResult<Response> {
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
    let verified = tokio::task::spawn_blocking(move || {
        record.filter(|r| crypto::verify_password(&password, &r.password_hash)).map(|r| r.user)
    })
    .await
    .map_err(AppError::internal)?;
    match verified {
        Some(user) => {
            s.login_failures.lock().unwrap().remove(&key);
            start_session(&s, user, &headers).await
        }
        None => {
            let mut failures = s.login_failures.lock().unwrap();
            let now = db::now();
            let entry = failures.entry(key).or_insert((0, now));
            if now - entry.1 >= LOGIN_LOCKOUT_SECS {
                entry.0 = 0;
            }
            entry.0 += 1;
            entry.1 = now;
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

async fn create_provider(
    State(s): State<AppState>,
    a: AuthUser,
    Json(input): Json<ProviderInput>,
) -> AppResult<Json<Provider>> {
    let (name, base_url) = clean_provider_input(&input)?;
    let key = input.api_key.as_deref().and_then(|k| encrypt_key(&s, k));
    let uid = a.user.id;
    let provider =
        s.db.call(move |c| {
            let id = db::insert_provider(c, uid, &name, input.kind, &base_url, key)?;
            db::get_provider(c, uid, id)
        })
        .await?
        .ok_or_else(|| AppError::internal("provider vanished after insert"))?;
    Ok(Json(provider.provider))
}

async fn update_provider(
    State(s): State<AppState>,
    a: AuthUser,
    Path(id): Path<i64>,
    Json(input): Json<ProviderInput>,
) -> AppResult<Json<Provider>> {
    let (name, base_url) = clean_provider_input(&input)?;
    let key = input.api_key.as_deref().map(|k| encrypt_key(&s, k));
    let uid = a.user.id;
    let provider =
        s.db.call(move |c| {
            db::update_provider(c, uid, id, &name, input.kind, &base_url, key)?;
            db::get_provider(c, uid, id)
        })
        .await?
        .ok_or_else(|| AppError::not_found("Provider"))?;
    Ok(Json(provider.provider))
}

async fn delete_provider(State(s): State<AppState>, a: AuthUser, Path(id): Path<i64>) -> AppResult<StatusCode> {
    if s.db.call(move |c| db::delete_provider(c, a.user.id, id)).await? {
        Ok(no_content())
    } else {
        Err(AppError::not_found("Provider"))
    }
}

async fn endpoint_for(s: &AppState, user_id: i64, provider_id: i64) -> AppResult<Endpoint> {
    let rec =
        s.db.call(move |c| db::get_provider(c, user_id, provider_id))
            .await?
            .ok_or_else(|| AppError::not_found("Provider"))?;
    let api_key = match rec.api_key_enc {
        Some(blob) => Some(s.vault.decrypt(&blob).map_err(|e| AppError::bad_request(e.to_string()))?),
        None => None,
    };
    Ok(Endpoint { kind: rec.provider.kind, base_url: rec.provider.base_url, api_key })
}

async fn provider_models(State(s): State<AppState>, a: AuthUser, Path(id): Path<i64>) -> AppResult<Json<ModelList>> {
    let ep = endpoint_for(&s, a.user.id, id).await?;
    let models = llm::list_models(&s.http, &ep).await.map_err(AppError::bad_gateway)?;
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

const DEFAULT_TITLE: &str = "New chat";

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

fn clean_title(t: &str) -> String {
    t.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(120).collect()
}

fn validate_params(p: &ChatParams) -> AppResult<()> {
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

async fn delete_conversation(State(s): State<AppState>, a: AuthUser, Path(id): Path<i64>) -> AppResult<StatusCode> {
    if s.db.call(move |c| db::delete_conversation(c, a.user.id, id)).await? {
        Ok(no_content())
    } else {
        Err(AppError::not_found("Conversation"))
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

// ---------- chat streaming ----------

async fn send_message(
    State(s): State<AppState>,
    a: AuthUser,
    Path(conv_id): Path<i64>,
    Json(req): Json<SendMessage>,
) -> AppResult<Response> {
    let content = req.content.trim().to_string();
    if content.is_empty() {
        return Err(AppError::bad_request("Message is empty"));
    }
    if content.chars().count() > MAX_MESSAGE_CHARS {
        return Err(AppError::bad_request("Message is too long"));
    }
    generate(s, a.user.id, conv_id, req.provider_id, req.model, req.params, Some((content, req.replace_from))).await
}

async fn regenerate(
    State(s): State<AppState>,
    a: AuthUser,
    Path(conv_id): Path<i64>,
    Json(req): Json<Regenerate>,
) -> AppResult<Response> {
    generate(s, a.user.id, conv_id, req.provider_id, req.model, req.params, None).await
}

/// Stores the user's message (if any), then streams the assistant's answer as SSE while saving it.
///
/// Generation runs in its own task: if the client disconnects (stop button, closed tab) the task
/// notices on the next chunk, stops calling the provider and saves what it has so far.
async fn generate(
    s: AppState,
    uid: i64,
    conv_id: i64,
    provider_id: i64,
    model: String,
    params: ChatParams,
    new_message: Option<(String, Option<i64>)>,
) -> AppResult<Response> {
    let model = model.trim().to_string();
    if model.is_empty() {
        return Err(AppError::bad_request("Pick a model first"));
    }
    validate_params(&params)?;
    let ep = endpoint_for(&s, uid, provider_id).await?;

    let (model_c, params_c) = (model.clone(), params.clone());
    let prepared =
        s.db.call(move |c| {
            let tx = c.transaction()?;
            let Some(conv) = db::get_conversation(&tx, uid, conv_id)? else { return Ok(None) };
            let user_message = match &new_message {
                Some((content, replace_from)) => {
                    if let Some(from) = replace_from {
                        db::truncate_from(&tx, uid, conv_id, *from)?;
                    }
                    if conv.title == DEFAULT_TITLE && conv.messages.is_empty() {
                        db::rename_conversation(&tx, uid, conv_id, &title_from(content))?;
                    }
                    Some(db::insert_message(&tx, conv_id, MsgRole::User, content, None, None)?)
                }
                None => {
                    // Regenerate: drop the assistant replies after the last user message.
                    match conv.messages.iter().rposition(|m| m.role == MsgRole::User) {
                        Some(i) => {
                            if let Some(next) = conv.messages.get(i + 1) {
                                db::truncate_from(&tx, uid, conv_id, next.id)?;
                            }
                        }
                        None => return Ok(Some(Err("There is no message to answer yet"))),
                    }
                    None
                }
            };
            db::set_conversation_model(&tx, uid, conv_id, provider_id, &model_c)?;
            db::set_conversation_params(&tx, uid, conv_id, &params_c)?;
            let history = db::get_conversation(&tx, uid, conv_id)?.map(|c| c.messages).unwrap_or_default();
            let assistant =
                db::insert_message(&tx, conv_id, MsgRole::Assistant, "", Some(provider_id), Some(&model_c))?;
            tx.commit()?;
            Ok(Some(Ok((user_message, history, assistant.id))))
        })
        .await?
        .ok_or_else(|| AppError::not_found("Conversation"))?
        .map_err(AppError::bad_request)?;
    let (user_message, history, assistant_id) = prepared;

    let turns: Vec<ChatTurn> = history
        .into_iter()
        .filter(|m| m.role == MsgRole::User || m.error.is_none() || m.error.as_deref() == Some(STOPPED))
        .map(|m| ChatTurn { role: m.role, content: m.content })
        .collect();

    let (tx, rx) = tokio::sync::mpsc::channel::<StreamEvent>(128);
    let _ = tx.try_send(StreamEvent::Start { user_message, assistant_message_id: assistant_id, model: model.clone() });

    tokio::spawn(async move {
        let mut text = String::new();
        let mut error: Option<String> = None;
        match llm::stream_chat(&s.http, &ep, &model, &turns, &params).await {
            Err(e) => error = Some(e),
            Ok(mut stream) => {
                while let Some(item) = stream.next().await {
                    match item {
                        Ok(delta) => {
                            text.push_str(&delta);
                            if tx.send(StreamEvent::Delta { text: delta }).await.is_err() {
                                error = Some(STOPPED.into());
                                break;
                            }
                        }
                        Err(e) => {
                            error = Some(e);
                            break;
                        }
                    }
                }
            }
        }
        if error.is_none() && text.trim().is_empty() {
            error = Some("The model returned an empty response".into());
        }
        let err_c = error.clone();
        let saved = s.db.call(move |c| db::finish_message(c, assistant_id, &text, err_c.as_deref())).await;
        match saved {
            Ok(Some(message)) => {
                if let Some(e) = error.filter(|e| e != STOPPED) {
                    let _ = tx.send(StreamEvent::Error { message: e }).await;
                }
                let _ = tx.send(StreamEvent::Done { message }).await;
            }
            Ok(None) => {} // conversation deleted mid-stream
            Err(e) => {
                tracing::error!("saving assistant message: {e}");
                let _ = tx.send(StreamEvent::Error { message: "Could not save the response".into() }).await;
            }
        }
    });

    let stream = event_stream(rx);
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))).into_response())
}

fn event_stream(mut rx: tokio::sync::mpsc::Receiver<StreamEvent>) -> impl Stream<Item = Result<Event, Infallible>> {
    futures_util::stream::poll_fn(move |cx| rx.poll_recv(cx))
        .map(|ev| Ok(Event::default().data(serde_json::to_string(&ev).unwrap_or_else(|_| "{}".into()))))
}

fn title_from(content: &str) -> String {
    let flat = clean_title(content);
    let mut title: String = flat.chars().take(60).collect();
    if flat.chars().count() > 60 {
        title.push('…');
    }
    if title.is_empty() {
        DEFAULT_TITLE.to_string()
    } else {
        title
    }
}

// ---------- devices ----------

async fn devices_info(State(s): State<AppState>, a: AuthUser) -> AppResult<Json<DevicesInfo>> {
    let urls = devices::reachable_urls(s.config.bind);
    // A QR code only helps when another device can actually reach the address.
    let qr_svg = urls.first().filter(|u| !u.contains("localhost")).and_then(|u| devices::qr_svg(u));
    let sessions = s.db.call(move |c| db::list_sessions(c, a.user.id, a.session_id)).await?;
    Ok(Json(DevicesInfo { urls, qr_svg, sessions }))
}

async fn revoke_device(State(s): State<AppState>, a: AuthUser, Path(id): Path<i64>) -> AppResult<StatusCode> {
    if id == a.session_id {
        return Err(AppError::bad_request("Use “Sign out” to end this device's session"));
    }
    if s.db.call(move |c| db::delete_session_by_id(c, a.user.id, id)).await? {
        Ok(no_content())
    } else {
        Err(AppError::not_found("Device"))
    }
}

async fn revoke_other_devices(State(s): State<AppState>, a: AuthUser) -> AppResult<StatusCode> {
    s.db.call(move |c| db::delete_other_sessions(c, a.user.id, a.session_id)).await?;
    Ok(no_content())
}
