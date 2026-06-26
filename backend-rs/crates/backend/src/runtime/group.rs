//! Group message streaming runtime.
//!
//! One turn = one user message fanned out to the agents a group routes it to.
//! The runtime persists the user message, selects responders, streams each
//! agent's reply token-by-token, and emits a terminal event. Ordering uses the
//! per-stream monotonic sequence on [`StreamEvent`] (never timestamps); durable
//! rows draw their thread sequence from [`SequenceAllocator`].
//!
//! Routing is intentionally simple and explicit:
//! 1. Explicit `@mentions` matching active group agents win outright.
//! 2. Otherwise, if the group is in free-speech or proactive mode, every active
//!    agent responds (joined-at order).
//! 3. Otherwise no agent responds and the turn ends in `silence`.
//!
//! In proactive mode an agent may decline its turn by replying with the silent
//! marker; that turn emits `agent_silent` and persists no agent message. An
//! agent may also pause the turn for human input via the waiting marker, which
//! emits `waiting_for_user` and stops the remaining proactive fan-out.
//!
//! Cancellation is cooperative: every event is pushed through an mpsc channel,
//! and the moment the receiver (the HTTP response body) is dropped, the next
//! send fails and the turn stops without emitting or persisting anything more.

use std::collections::HashSet;
use std::sync::Arc;

use ag_swarmer_domain::events::{StreamEvent, StreamEventKind};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::mpsc::Sender;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::llm::{
    AnthropicProvider, ChatDelta, ChatMessage, ChatRequest, GeminiProvider, LlmProvider,
    OpenAiCompatibleProvider,
};
use crate::runtime::sequence::{NewMessage, SequenceAllocator};

/// A proactive agent replies with exactly this marker to stay silent.
pub const SILENT_MARKER: &str = "<SILENT>";
/// An agent prefixes its reply with this marker to pause for human input.
pub const WAITING_MARKER: &str = "<WAITING_FOR_USER>";

/// Shared services the group runtime needs to read config and persist state.
#[derive(Clone)]
pub struct RuntimeServices {
    pub pool: SqlitePool,
    pub write_lock: Arc<Mutex<()>>,
}

impl RuntimeServices {
    pub fn new(pool: SqlitePool, write_lock: Arc<Mutex<()>>) -> Self {
        Self { pool, write_lock }
    }

    fn allocator(&self) -> SequenceAllocator {
        SequenceAllocator::new(self.pool.clone(), self.write_lock.clone())
    }
}

/// A single group turn to run.
pub struct TurnRequest {
    pub group_id: String,
    pub owner_id: String,
    pub thread_id: Option<String>,
    pub content: String,
}

/// How a turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnOutcome {
    /// At least one agent produced a visible message.
    Completed,
    /// No agent spoke (none routed, or all proactive agents stayed silent).
    Silence,
    /// An agent paused the turn for human input.
    WaitingForUser,
    /// The client disconnected mid-stream.
    Cancelled,
    /// A configuration or provider error ended the turn.
    Error,
}

/// Marker that the receiving end of the stream has gone away.
struct Cancelled;

/// A step failed either because the client vanished or because a write errored.
enum StepErr {
    Cancelled,
    Db(anyhow::Error),
}

/// Run one group turn, pushing every [`StreamEvent`] through `tx`.
///
/// Returns once the turn reaches a terminal event, errors, or the receiver is
/// dropped (`TurnOutcome::Cancelled`).
pub async fn run_group_turn(
    services: RuntimeServices,
    req: TurnRequest,
    tx: Sender<StreamEvent<Value>>,
) -> TurnOutcome {
    let stream_id = Uuid::new_v4();

    // Resolve the thread before building the streaming context: a bad thread id
    // is reported as an `error`/`done` pair on a fresh stream.
    let thread_id = match resolve_or_create_thread(&services, &req).await {
        Ok(id) => id,
        Err(err) => {
            let error = StreamEvent::new(
                stream_id,
                0,
                StreamEventKind::Error,
                json!({ "message": err.to_string() }),
            );
            if tx.send(error).await.is_err() {
                return TurnOutcome::Cancelled;
            }
            let done = StreamEvent::new(stream_id, 1, StreamEventKind::Done, json!({}));
            let _ = tx.send(done).await;
            return TurnOutcome::Error;
        }
    };

    let mut ctx = StreamCtx {
        stream_id,
        seq: 0,
        tx,
        allocator: services.allocator(),
        thread_id,
        group_id: req.group_id.clone(),
    };

    match run_inner(&services, &req, &mut ctx).await {
        Ok(outcome) => outcome,
        Err(Cancelled) => TurnOutcome::Cancelled,
    }
}

/// Per-stream emit state: the stream id, the monotonic sequence counter, the
/// outbound channel, and the durable-write allocator.
struct StreamCtx {
    stream_id: Uuid,
    seq: i64,
    tx: Sender<StreamEvent<Value>>,
    allocator: SequenceAllocator,
    thread_id: String,
    group_id: String,
}

impl StreamCtx {
    fn next_event(&mut self, kind: StreamEventKind, payload: Value) -> StreamEvent<Value> {
        let event = StreamEvent::new(self.stream_id, self.seq, kind, payload);
        self.seq += 1;
        event
    }

    /// Emit an ephemeral event (tokens, reasoning, lifecycle markers). Not
    /// persisted.
    async fn emit(&mut self, kind: StreamEventKind, payload: Value) -> Result<(), StepErr> {
        let event = self.next_event(kind, payload);
        self.tx.send(event).await.map_err(|_| StepErr::Cancelled)
    }

    /// Persist a message and its announcing event, then emit it.
    async fn emit_message(
        &mut self,
        kind: StreamEventKind,
        payload: Value,
        message: &NewMessage,
    ) -> Result<(), StepErr> {
        let event = self.next_event(kind, payload);
        self.allocator
            .persist_message_with_event(&self.thread_id, &self.group_id, message, &event)
            .await
            .map_err(StepErr::Db)?;
        self.tx.send(event).await.map_err(|_| StepErr::Cancelled)
    }

    /// Persist a durable event with no message row, then emit it.
    async fn emit_durable_event(
        &mut self,
        kind: StreamEventKind,
        payload: Value,
    ) -> Result<(), StepErr> {
        let event = self.next_event(kind, payload);
        self.allocator
            .persist_event(&self.thread_id, &event)
            .await
            .map_err(StepErr::Db)?;
        self.tx.send(event).await.map_err(|_| StepErr::Cancelled)
    }

    /// Emit an `error` then `done` and finish the turn as `Error`. Propagates
    /// `Cancelled` if the client has already gone.
    async fn fail(&mut self, message: &str) -> Result<TurnOutcome, Cancelled> {
        if self
            .emit(StreamEventKind::Error, json!({ "message": message }))
            .await
            .is_err()
        {
            return Err(Cancelled);
        }
        let _ = self.emit(StreamEventKind::Done, json!({})).await;
        Ok(TurnOutcome::Error)
    }
}

/// Run a step expression, short-circuiting on cancellation or DB failure.
macro_rules! step {
    ($ctx:expr, $expr:expr) => {
        match $expr {
            Ok(value) => value,
            Err(StepErr::Cancelled) => return Ok(TurnOutcome::Cancelled),
            Err(StepErr::Db(err)) => return $ctx.fail(&err.to_string()).await,
        }
    };
}

async fn run_inner(
    services: &RuntimeServices,
    req: &TurnRequest,
    ctx: &mut StreamCtx,
) -> Result<TurnOutcome, Cancelled> {
    let (free_speech, proactive) = match load_group_flags(&services.pool, &req.group_id).await {
        Ok(flags) => flags,
        Err(err) => return ctx.fail(&err.to_string()).await,
    };

    // 1. Persist the user message and announce it.
    let user_message = NewMessage {
        id: Uuid::new_v4().to_string(),
        sender_type: "user".to_string(),
        sender_id: Some(req.owner_id.clone()),
        message_type: "text".to_string(),
        content: req.content.clone(),
    };
    let user_payload = json!({
        "message_id": user_message.id,
        "thread_id": ctx.thread_id,
        "content": req.content,
        "sender_type": "user",
    });
    step!(
        ctx,
        ctx.emit_message(StreamEventKind::UserMessage, user_payload, &user_message)
            .await
    );

    // 2. Route the message to responders.
    let candidates = match load_candidates(&services.pool, &req.group_id).await {
        Ok(candidates) => candidates,
        Err(err) => return ctx.fail(&err.to_string()).await,
    };
    let selected = select_agents(candidates, &req.content, free_speech, proactive);

    if selected.is_empty() {
        step!(
            ctx,
            ctx.emit_durable_event(StreamEventKind::Silence, json!({}))
                .await
        );
        step!(ctx, ctx.emit(StreamEventKind::Done, json!({})).await);
        return Ok(TurnOutcome::Silence);
    }

    // 3. Fan out to each selected agent, sequentially.
    let mut had_visible = false;
    let mut waiting = false;

    for agent in &selected {
        step!(
            ctx,
            ctx.emit(
                StreamEventKind::AgentStart,
                json!({ "agent_id": agent.agent_id, "display_name": agent.display_name }),
            )
            .await
        );

        let provider_cfg = match resolve_provider(&services.pool, agent).await {
            Ok(cfg) => cfg,
            Err(err) => return ctx.fail(&err.to_string()).await,
        };
        let provider = match build_provider(&provider_cfg) {
            Ok(provider) => provider,
            Err(err) => return ctx.fail(&err.to_string()).await,
        };
        let model = model_from_config(&agent.model_config_json, &provider_cfg.default_model);
        let messages =
            match build_messages(&services.pool, &ctx.thread_id, &agent.system_prompt).await {
                Ok(messages) => messages,
                Err(err) => return ctx.fail(&err.to_string()).await,
            };
        let request = ChatRequest {
            model,
            messages,
            temperature: None,
            reasoning_passback: provider_cfg.reasoning_passback,
        };
        let mut deltas = match provider.stream(request).await {
            Ok(rx) => rx,
            Err(err) => return ctx.fail(&err.to_string()).await,
        };

        let mut content = String::new();
        while let Some(delta) = deltas.recv().await {
            match delta {
                ChatDelta::Token(text) => {
                    content.push_str(&text);
                    step!(
                        ctx,
                        ctx.emit(
                            StreamEventKind::Token,
                            json!({ "agent_id": agent.agent_id, "text": text }),
                        )
                        .await
                    );
                }
                ChatDelta::Reasoning(text) => {
                    step!(
                        ctx,
                        ctx.emit(
                            StreamEventKind::Reasoning,
                            json!({ "agent_id": agent.agent_id, "text": text }),
                        )
                        .await
                    );
                }
                ChatDelta::ToolCall(call) => {
                    step!(
                        ctx,
                        ctx.emit(
                            StreamEventKind::ToolCallStart,
                            json!({
                                "agent_id": agent.agent_id,
                                "tool_call_id": call.id,
                                "name": call.name,
                                "args": call.args,
                            }),
                        )
                        .await
                    );
                }
                ChatDelta::Usage(usage) => {
                    step!(
                        ctx,
                        ctx.emit(
                            StreamEventKind::ContextUsage,
                            json!({
                                "agent_id": agent.agent_id,
                                "input_tokens": usage.input_tokens,
                                "output_tokens": usage.output_tokens,
                                "total_tokens": usage.total_tokens,
                            }),
                        )
                        .await
                    );
                }
                ChatDelta::Done => break,
            }
        }

        let trimmed = content.trim();

        // Proactive silence: emit the marker event, persist no message.
        if proactive && trimmed == SILENT_MARKER {
            step!(
                ctx,
                ctx.emit_durable_event(
                    StreamEventKind::AgentSilent,
                    json!({ "agent_id": agent.agent_id }),
                )
                .await
            );
            continue;
        }

        let is_waiting = trimmed.starts_with(WAITING_MARKER);
        let visible = if is_waiting {
            let rest = trimmed[WAITING_MARKER.len()..].trim();
            if rest.is_empty() {
                "Waiting for your input".to_string()
            } else {
                rest.to_string()
            }
        } else {
            content.clone()
        };

        let agent_message = NewMessage {
            id: Uuid::new_v4().to_string(),
            sender_type: "agent".to_string(),
            sender_id: Some(agent.agent_id.clone()),
            message_type: "text".to_string(),
            content: visible.clone(),
        };
        let message_payload = json!({
            "message_id": agent_message.id,
            "agent_id": agent.agent_id,
            "content": visible,
        });
        step!(
            ctx,
            ctx.emit_message(
                StreamEventKind::AgentMessage,
                message_payload,
                &agent_message
            )
            .await
        );
        had_visible = true;

        if is_waiting {
            step!(
                ctx,
                ctx.emit_durable_event(
                    StreamEventKind::WaitingForUser,
                    json!({ "agent_id": agent.agent_id, "message": visible }),
                )
                .await
            );
            waiting = true;
            break;
        }
    }

    // 4. Terminal event.
    if waiting {
        step!(ctx, ctx.emit(StreamEventKind::Done, json!({})).await);
        return Ok(TurnOutcome::WaitingForUser);
    }
    if !had_visible {
        step!(
            ctx,
            ctx.emit_durable_event(StreamEventKind::Silence, json!({}))
                .await
        );
        step!(ctx, ctx.emit(StreamEventKind::Done, json!({})).await);
        return Ok(TurnOutcome::Silence);
    }
    step!(ctx, ctx.emit(StreamEventKind::Done, json!({})).await);
    Ok(TurnOutcome::Completed)
}

/// An active agent eligible to respond in the group.
struct Candidate {
    agent_id: String,
    display_name: String,
    system_prompt: String,
    provider_id: Option<String>,
    model_config_json: Option<String>,
}

/// Resolved LLM provider connection settings.
struct ProviderConfig {
    kind: String,
    base_url: Option<String>,
    api_key: String,
    default_model: String,
    reasoning_passback: bool,
}

async fn load_group_flags(pool: &SqlitePool, group_id: &str) -> anyhow::Result<(bool, bool)> {
    let row: Option<(i64, i64)> =
        sqlx::query_as("SELECT free_speech, proactive_mode FROM groups WHERE id = ?")
            .bind(group_id)
            .fetch_optional(pool)
            .await?;
    let (free_speech, proactive) = row.ok_or_else(|| anyhow::anyhow!("group not found"))?;
    Ok((free_speech != 0, proactive != 0))
}

#[derive(sqlx::FromRow)]
struct CandidateRow {
    id: String,
    display_name: Option<String>,
    name: String,
    system_prompt: String,
    provider_id: Option<String>,
    model_config_json: Option<String>,
}

async fn load_candidates(pool: &SqlitePool, group_id: &str) -> anyhow::Result<Vec<Candidate>> {
    let rows: Vec<CandidateRow> = sqlx::query_as(
        "SELECT a.id, ga.display_name, a.name, a.system_prompt, a.provider_id, \
                a.model_config_json \
         FROM group_agents ga \
         JOIN agents a ON a.id = ga.agent_id \
         WHERE ga.group_id = ? AND ga.status = 'active' AND a.status = 'active' \
         ORDER BY ga.joined_at ASC, a.id ASC",
    )
    .bind(group_id)
    .fetch_all(pool)
    .await?;

    // First-match-wins when two agents share an effective display name.
    let mut seen_names: HashSet<String> = HashSet::new();
    let mut candidates = Vec::new();
    for row in rows {
        let display = row.display_name.unwrap_or(row.name);
        if !seen_names.insert(display.to_lowercase()) {
            continue;
        }
        candidates.push(Candidate {
            agent_id: row.id,
            display_name: display,
            system_prompt: row.system_prompt,
            provider_id: row.provider_id,
            model_config_json: row.model_config_json,
        });
    }
    Ok(candidates)
}

/// Pick the responders for `text`: explicit mentions win; otherwise free-speech
/// or proactive mode fans out to everyone; otherwise nobody.
fn select_agents(
    candidates: Vec<Candidate>,
    text: &str,
    free_speech: bool,
    proactive: bool,
) -> Vec<Candidate> {
    if text.contains('@') {
        let mentioned = scan_mentions(text, &candidates);
        if !mentioned.is_empty() {
            // Reorder candidates into mention (textual) order, keeping only those
            // mentioned.
            let mut by_index: Vec<Option<Candidate>> = candidates.into_iter().map(Some).collect();
            return mentioned
                .into_iter()
                .filter_map(|index| by_index[index].take())
                .collect();
        }
    }
    if free_speech || proactive {
        return candidates;
    }
    Vec::new()
}

/// Walk `text` left-to-right and return the candidate indices that are
/// `@mentioned`, in textual order, deduplicated. Longest display name wins at
/// each `@` so `@echo` does not shadow `@echolike`.
fn scan_mentions(text: &str, candidates: &[Candidate]) -> Vec<usize> {
    if candidates.is_empty() {
        return Vec::new();
    }
    // (candidate index, lowercase name chars), longest first.
    let mut names: Vec<(usize, Vec<char>)> = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            (
                index,
                candidate.display_name.to_lowercase().chars().collect(),
            )
        })
        .collect();
    names.sort_by_key(|(_, chars)| std::cmp::Reverse(chars.len()));

    let chars: Vec<char> = text.chars().collect();
    let lower: Vec<char> = text.to_lowercase().chars().collect();
    let len = chars.len();

    let mut out = Vec::new();
    let mut seen: HashSet<usize> = HashSet::new();
    let mut i = 0;
    while i < len {
        if chars[i] != '@' {
            i += 1;
            continue;
        }
        let mut matched = false;
        for (index, name) in &names {
            let end = i + 1 + name.len();
            if end > lower.len() || &lower[i + 1..end] != name.as_slice() {
                continue;
            }
            // The match must end at a name boundary.
            if end != len && is_name_char(chars[end]) {
                continue;
            }
            if seen.insert(*index) {
                out.push(*index);
            }
            i = end;
            matched = true;
            break;
        }
        if !matched {
            i += 1;
        }
    }
    out
}

fn is_name_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '-' || ('\u{4e00}'..='\u{9fff}').contains(&ch)
}

async fn resolve_provider(pool: &SqlitePool, agent: &Candidate) -> anyhow::Result<ProviderConfig> {
    let provider_id = agent
        .provider_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("agent has no llm provider configured"))?;
    let row: Option<(String, Option<String>, String, String, i64)> = sqlx::query_as(
        "SELECT kind, base_url, api_key, default_model, reasoning_passback \
         FROM llm_providers WHERE id = ? AND status = 'active'",
    )
    .bind(provider_id)
    .fetch_optional(pool)
    .await?;
    let (kind, base_url, api_key, default_model, reasoning_passback) =
        row.ok_or_else(|| anyhow::anyhow!("agent llm provider not found"))?;
    Ok(ProviderConfig {
        kind,
        base_url,
        api_key,
        default_model,
        reasoning_passback: reasoning_passback != 0,
    })
}

fn build_provider(cfg: &ProviderConfig) -> anyhow::Result<Box<dyn LlmProvider>> {
    let base_url = cfg.base_url.clone().unwrap_or_default();
    let provider: Box<dyn LlmProvider> = match cfg.kind.as_str() {
        "openai_compatible" | "openai" | "deepseek" | "vllm" | "openrouter" => {
            Box::new(OpenAiCompatibleProvider::new(base_url, cfg.api_key.clone()))
        }
        "anthropic" => Box::new(AnthropicProvider::new(base_url, cfg.api_key.clone())),
        "gemini" | "google" => Box::new(GeminiProvider::new(base_url, cfg.api_key.clone())),
        other => anyhow::bail!("unsupported provider kind: {other}"),
    };
    Ok(provider)
}

fn model_from_config(model_config_json: &Option<String>, default_model: &str) -> String {
    model_config_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|value| {
            value
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| default_model.to_string())
}

async fn build_messages(
    pool: &SqlitePool,
    thread_id: &str,
    system_prompt: &str,
) -> anyhow::Result<Vec<ChatMessage>> {
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT sender_type, content FROM messages \
         WHERE thread_id = ? AND status = 'visible' ORDER BY seq ASC",
    )
    .bind(thread_id)
    .fetch_all(pool)
    .await?;

    let mut messages = vec![ChatMessage {
        role: "system".to_string(),
        content: system_prompt.to_string(),
    }];
    for (sender_type, content) in rows {
        let role = if sender_type == "agent" {
            "assistant"
        } else {
            "user"
        };
        messages.push(ChatMessage {
            role: role.to_string(),
            content: content.unwrap_or_default(),
        });
    }
    Ok(messages)
}

/// Resolve the turn's thread: validate a supplied id, reuse the active group
/// thread, or create one. Creation is serialized behind the write lock and
/// re-checks for a race winner.
async fn resolve_or_create_thread(
    services: &RuntimeServices,
    req: &TurnRequest,
) -> anyhow::Result<String> {
    if let Some(thread_id) = &req.thread_id {
        let row: Option<(String, String, String)> =
            sqlx::query_as("SELECT id, group_id, status FROM threads WHERE id = ?")
                .bind(thread_id)
                .fetch_optional(&services.pool)
                .await?;
        return match row {
            Some((id, group_id, status)) if group_id == req.group_id && status == "active" => {
                Ok(id)
            }
            Some(_) => Err(anyhow::anyhow!(
                "thread is not an active thread of this group"
            )),
            None => Err(anyhow::anyhow!("thread not found")),
        };
    }

    if let Some(id) = active_group_thread(&services.pool, &req.group_id).await? {
        return Ok(id);
    }

    let _guard = services.write_lock.lock().await;
    if let Some(id) = active_group_thread(&services.pool, &req.group_id).await? {
        return Ok(id);
    }
    let id = Uuid::new_v4().to_string();
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO threads (id, group_id, agent_id, status, next_seq, created_at, updated_at) \
         VALUES (?, ?, NULL, 'active', 1, ?, ?)",
    )
    .bind(&id)
    .bind(&req.group_id)
    .bind(&now)
    .bind(&now)
    .execute(&services.pool)
    .await?;
    Ok(id)
}

async fn active_group_thread(pool: &SqlitePool, group_id: &str) -> anyhow::Result<Option<String>> {
    let id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM threads \
         WHERE group_id = ? AND agent_id IS NULL AND status = 'active' \
         ORDER BY created_at ASC, id ASC LIMIT 1",
    )
    .bind(group_id)
    .fetch_optional(pool)
    .await?;
    Ok(id)
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}
