//! Sending messages and streaming answers, including the tool-use loop.

use std::{collections::HashMap, convert::Infallible, time::Duration};

use axum::{
    extract::{Path, State},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Json,
};
use futures_util::{Stream, StreamExt};
use neko_shared::*;

use crate::{
    api::{clean_title, endpoint_for, validate_params, DEFAULT_TITLE},
    auth::AuthUser,
    db,
    error::{AppError, AppResult},
    llm::{self, CallResult, LlmEvent, Turn},
    tools::{web::truncate, Toolbox},
    uploads, AppState,
};

const MAX_MESSAGE_CHARS: usize = 200_000;
const MAX_ATTACHMENTS: usize = 10;
/// Model ↔ tool round trips per answer. The last step offers no tools so the model must reply.
const MAX_STEPS: usize = 8;
const TOOL_TIMEOUT: Duration = Duration::from_secs(120);
/// How much of each tool result is kept for display and for later turns.
const TOOL_RESULT_DISPLAY: usize = 4_000;
const TOOL_RESULT_HISTORY: usize = 1_500;
/// Error marker for answers the user stopped; their partial text is still sent as context.
pub const STOPPED: &str = "Stopped";

pub async fn send_message(
    State(s): State<AppState>,
    a: AuthUser,
    Path(conv_id): Path<i64>,
    Json(req): Json<SendMessage>,
) -> AppResult<Response> {
    let content = req.content.trim().to_string();
    if content.is_empty() && req.attachments.is_empty() {
        return Err(AppError::bad_request("Message is empty"));
    }
    if content.chars().count() > MAX_MESSAGE_CHARS {
        return Err(AppError::bad_request("Message is too long"));
    }
    if req.attachments.len() > MAX_ATTACHMENTS {
        return Err(AppError::bad_request("At most 10 files per message"));
    }
    let new = NewMessage { content, replace_from: req.replace_from, attachments: req.attachments };
    generate(s, a.user, conv_id, req.provider_id, req.model, req.params, req.tools, Some(new)).await
}

pub async fn regenerate(
    State(s): State<AppState>,
    a: AuthUser,
    Path(conv_id): Path<i64>,
    Json(req): Json<Regenerate>,
) -> AppResult<Response> {
    generate(s, a.user, conv_id, req.provider_id, req.model, req.params, req.tools, None).await
}

struct NewMessage {
    content: String,
    replace_from: Option<i64>,
    attachments: Vec<i64>,
}

/// Turns stored history into provider-neutral turns. Earlier tool use is summarized as text, so
/// switching providers mid-chat never trips over another provider's tool-call format.
async fn history_turns(
    s: &AppState,
    history: &[Message],
    files: &HashMap<i64, Vec<db::AttachmentRecord>>,
) -> Vec<Turn> {
    let mut turns = Vec::new();
    for m in history {
        match m.role {
            MsgRole::User => {
                let media = match files.get(&m.id) {
                    Some(recs) => uploads::load_media(&s.config.data_dir, recs).await,
                    None => Vec::new(),
                };
                turns.push(Turn::User { text: m.content.clone(), media });
            }
            MsgRole::Assistant => {
                if m.error.is_some() && m.error.as_deref() != Some(STOPPED) {
                    continue;
                }
                let mut text = String::new();
                for c in &m.tool_calls {
                    if c.name.is_empty() || !c.done {
                        continue;
                    }
                    text.push_str(&format!(
                        "[I used the tool {} with {}. Result: {}]\n\n",
                        c.label,
                        truncate(&c.arguments, 300),
                        truncate(&c.result, TOOL_RESULT_HISTORY)
                    ));
                }
                text.push_str(&m.content);
                turns.push(Turn::assistant(text));
            }
        }
    }
    turns
}

/// Stores the user's message (if any), then streams the assistant's answer as SSE while saving it.
///
/// Generation runs in its own task: if the client disconnects (stop button, closed tab) the task
/// notices on the next chunk, stops calling the provider and saves what it has so far.
#[allow(clippy::too_many_arguments)]
async fn generate(
    s: AppState,
    user: User,
    conv_id: i64,
    provider_id: i64,
    model: String,
    params: ChatParams,
    tool_sel: ToolSelection,
    new_message: Option<NewMessage>,
) -> AppResult<Response> {
    let model = model.trim().to_string();
    if model.is_empty() {
        return Err(AppError::bad_request("Pick a model first"));
    }
    validate_params(&params)?;
    let uid = user.id;
    let ep = endpoint_for(&s, uid, provider_id).await?;

    let (model_c, params_c) = (model.clone(), params.clone());
    let prepared =
        s.db.call(move |c| {
            let tx = c.transaction()?;
            let Some(conv) = db::get_conversation(&tx, uid, conv_id)? else { return Ok(None) };
            let user_message = match &new_message {
                Some(nm) => {
                    if let Some(from) = nm.replace_from {
                        db::truncate_from(&tx, uid, conv_id, from)?;
                    }
                    if conv.title == DEFAULT_TITLE && conv.messages.is_empty() {
                        db::rename_conversation(&tx, uid, conv_id, &title_from(&nm.content))?;
                    }
                    let mut msg = db::insert_message(&tx, conv_id, MsgRole::User, &nm.content, None, None)?;
                    if !nm.attachments.is_empty() {
                        let linked = db::link_attachments(&tx, uid, &nm.attachments, msg.id)?;
                        if linked != nm.attachments.len() {
                            return Ok(Some(Err(
                                "One of the attached files is no longer available. Remove it and try again.",
                            )));
                        }
                        msg.attachments = nm
                            .attachments
                            .iter()
                            .filter_map(|id| db::get_attachment(&tx, uid, *id).ok().flatten().map(|r| r.info))
                            .collect();
                    }
                    Some(msg)
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
            let files = db::conversation_attachments(&tx, conv_id)?;
            let assistant =
                db::insert_message(&tx, conv_id, MsgRole::Assistant, "", Some(provider_id), Some(&model_c))?;
            tx.commit()?;
            Ok(Some(Ok((user_message, history, files, assistant.id))))
        })
        .await?
        .ok_or_else(|| AppError::not_found("Conversation"))?
        .map_err(AppError::bad_request)?;
    let (user_message, history, files, assistant_id) = prepared;

    let mut turns = history_turns(&s, &history, &files).await;
    let (toolbox, warnings) = if tool_sel == ToolSelection::default() {
        (Toolbox::empty(), Vec::new())
    } else {
        Toolbox::build(&s, &user, &tool_sel).await
    };

    let (tx, rx) = tokio::sync::mpsc::channel::<StreamEvent>(128);
    let _ = tx.try_send(StreamEvent::Start { user_message, assistant_message_id: assistant_id, model: model.clone() });

    tokio::spawn(async move {
        let mut text = String::new();
        let mut error: Option<String> = None;
        let mut log: Vec<ToolCall> = Vec::new();

        for (label, problem) in warnings {
            let card = ToolCall {
                id: llm::new_call_id(),
                name: String::new(),
                label,
                arguments: "{}".into(),
                result: problem,
                is_error: true,
                done: true,
            };
            let _ = tx.send(StreamEvent::ToolEnd { call: card.clone() }).await;
            log.push(card);
        }

        'steps: for step in 0..MAX_STEPS {
            let tools = if step + 1 == MAX_STEPS { &[][..] } else { &toolbox.defs[..] };
            let mut stream = match llm::stream_chat(&s.http, &ep, &model, &turns, &params, tools).await {
                Ok(st) => st,
                Err(e) => {
                    error = Some(e);
                    break;
                }
            };
            let mut step_text = String::new();
            let mut calls = Vec::new();
            while let Some(item) = stream.next().await {
                match item {
                    Ok(LlmEvent::Text(delta)) => {
                        // Separate the text of successive steps.
                        let delta =
                            if step_text.is_empty() && !text.is_empty() { format!("\n\n{delta}") } else { delta };
                        step_text.push_str(&delta);
                        text.push_str(&delta);
                        if tx.send(StreamEvent::Delta { text: delta }).await.is_err() {
                            error = Some(STOPPED.into());
                            break 'steps;
                        }
                    }
                    Ok(LlmEvent::Call(c)) => calls.push(c),
                    Err(e) => {
                        error = Some(e);
                        break 'steps;
                    }
                }
            }
            if calls.is_empty() {
                break;
            }
            turns.push(Turn::Assistant { text: step_text, calls: calls.clone() });
            let mut results = Vec::new();
            for c in calls {
                let mut card = ToolCall {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    label: toolbox.label(&c.name),
                    arguments: c.arguments.to_string(),
                    result: String::new(),
                    is_error: false,
                    done: false,
                };
                if tx.send(StreamEvent::ToolStart { call: card.clone() }).await.is_err() {
                    error = Some(STOPPED.into());
                    break 'steps;
                }
                let run = tokio::time::timeout(TOOL_TIMEOUT, toolbox.run(&s, &c.name, &c.arguments));
                let (output, is_error) = tokio::select! {
                    r = run => r.unwrap_or_else(|_| ("The tool took too long and was cancelled".into(), true)),
                    _ = tx.closed() => {
                        card.result = STOPPED.into();
                        card.is_error = true;
                        card.done = true;
                        log.push(card);
                        error = Some(STOPPED.into());
                        break 'steps;
                    }
                };
                card.result = truncate(&output, TOOL_RESULT_DISPLAY);
                card.is_error = is_error;
                card.done = true;
                log.push(card.clone());
                if tx.send(StreamEvent::ToolEnd { call: card }).await.is_err() {
                    error = Some(STOPPED.into());
                    break 'steps;
                }
                results.push(CallResult { id: c.id, name: c.name, content: output, is_error });
            }
            turns.push(Turn::ToolResults(results));
        }

        if error.is_none() && text.trim().is_empty() {
            error = Some("The model returned an empty response".into());
        }
        let err_c = error.clone();
        let saved = s.db.call(move |c| db::finish_message(c, assistant_id, &text, err_c.as_deref(), &log)).await;
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
