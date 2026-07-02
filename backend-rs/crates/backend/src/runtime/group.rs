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
//! Stream delivery is best effort: every durable event is persisted before it is
//! pushed through the mpsc channel. If the HTTP response body is dropped, the
//! runtime keeps running to a replayable terminal state so reconnect can
//! converge from the client's last event id.

use std::sync::Arc;
use std::{collections::HashSet, path::PathBuf};

use ag_swarmer_domain::events::{StreamEvent, StreamEventKind};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::mpsc::Sender;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::acp::{normalize_acp_runtime, run_acp_agent_stream, AcpEventKind, AcpRunRequest};
use crate::llm::{
    AnthropicProvider, ChatDelta, ChatMessage, ChatRequest, GeminiProvider, LlmProvider,
    OpenAiCompatibleProvider, ToolCall, ToolDefinition,
};
use crate::runtime::agent_as_tool::{
    resolve_dispatch, AgentAsToolCall, AgentAsToolFailure, CallerAgent, AGENT_AS_TOOL_NAME,
};
use crate::tools::{MountedSkill, ToolExecutor, ToolResult, ToolStatus};

const MAX_TOOL_ROUNDS: usize = 24;
use crate::runtime::sequence::{NewMessage, SequenceAllocator};

/// A proactive agent replies with exactly this marker to stay silent.
pub const SILENT_MARKER: &str = "<SILENT>";
/// An agent prefixes its reply with this marker to pause for human input.
pub const WAITING_MARKER: &str = "<WAITING_FOR_USER>";

const RESUME_CONTINUATION_PROMPT: &str =
    "Continue from where you left off. Do not repeat completed text; append only the continuation.";

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

/// A request to continue the latest interrupted message in a paused thread.
pub struct ResumeRequest {
    pub group_id: String,
    pub thread_id: String,
    pub agent_id: String,
    pub message_id: String,
    pub existing_content: String,
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
    /// The client disconnected before any replayable thread state existed.
    Cancelled,
    /// A configuration or provider error ended the turn.
    Error,
}

/// Marker that the receiving end of the stream has gone away.
struct Cancelled;

/// A step failed either because the client vanished or because a write errored.
enum StepErr {
    #[allow(dead_code)]
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

/// Resume an interrupted agent message, appending newly streamed tokens to the
/// existing row rather than creating a replacement message.
pub async fn run_thread_resume(
    services: RuntimeServices,
    req: ResumeRequest,
    tx: Sender<StreamEvent<Value>>,
) -> TurnOutcome {
    let stream_id = Uuid::new_v4();
    let mut ctx = StreamCtx {
        stream_id,
        seq: 0,
        tx,
        allocator: services.allocator(),
        thread_id: req.thread_id.clone(),
        group_id: req.group_id.clone(),
    };

    match run_resume_inner(&services, &req, &mut ctx).await {
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

    /// Emit an event and persist its stream cursor before delivery so reconnect
    /// replay can anchor on any id the client may have observed.
    async fn emit(&mut self, kind: StreamEventKind, payload: Value) -> Result<(), StepErr> {
        let event = self.next_event(kind, payload);
        self.allocator
            .persist_event(&self.thread_id, &event)
            .await
            .map_err(StepErr::Db)?;
        let _ = self.tx.send(event).await;
        Ok(())
    }

    /// Persist a message and its announcing event before emitting it. Delivery
    /// failures are non-fatal because the persisted event can be replayed.
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
        let _ = self.tx.send(event).await;
        Ok(())
    }

    /// Update an existing interrupted message and persist both final durable
    /// events before emitting them.
    async fn emit_resume_completion(
        &mut self,
        payload: Value,
        message_id: &str,
        content: &str,
    ) -> Result<(), StepErr> {
        let message_event = self.next_event(StreamEventKind::AgentMessage, payload);
        let done_event = self.next_event(StreamEventKind::Done, json!({}));
        self.allocator
            .complete_interrupted_message_with_events(
                &self.thread_id,
                message_id,
                content,
                &message_event,
                &done_event,
            )
            .await
            .map_err(StepErr::Db)?;
        let _ = self.tx.send(message_event).await;
        let _ = self.tx.send(done_event).await;
        Ok(())
    }

    /// Persist a durable event with no message row before emitting it.
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
        let _ = self.tx.send(event).await;
        Ok(())
    }

    async fn emit_done(&mut self) -> Result<(), StepErr> {
        self.emit_durable_event(StreamEventKind::Done, json!({}))
            .await
    }

    /// Emit an `error` then `done` and finish the turn as `Error`.
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
    let group = match load_group_runtime_config(&services.pool, &req.group_id).await {
        Ok(config) => config,
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
    let candidates = match load_candidates(&services.pool, &req.group_id, &group).await {
        Ok(candidates) => candidates,
        Err(err) => return ctx.fail(&err.to_string()).await,
    };
    let selected = select_agents(candidates, &req.content, &group);

    if selected.is_empty() {
        step!(
            ctx,
            ctx.emit_durable_event(StreamEventKind::Silence, json!({}))
                .await
        );
        step!(ctx, ctx.emit_done().await);
        return Ok(TurnOutcome::Silence);
    }

    // 3. Fan out to each selected agent, sequentially.
    let mut had_visible = false;
    let mut waiting = false;

    for agent in &selected {
        match step!(ctx, run_agent_turn(services, ctx, agent, &group, 0).await) {
            AgentRunResult::NoVisible => {}
            AgentRunResult::Visible => had_visible = true,
            AgentRunResult::WaitingForUser => {
                had_visible = true;
                waiting = true;
                break;
            }
            AgentRunResult::Handoff { helper } => {
                had_visible = true;
                match step!(ctx, run_agent_turn(services, ctx, &helper, &group, 1).await) {
                    AgentRunResult::WaitingForUser => waiting = true,
                    AgentRunResult::Visible
                    | AgentRunResult::NoVisible
                    | AgentRunResult::Handoff { .. } => {}
                }
                break;
            }
        }
    }

    // 4. Terminal event.
    if waiting {
        step!(ctx, ctx.emit_done().await);
        return Ok(TurnOutcome::WaitingForUser);
    }
    if !had_visible {
        step!(
            ctx,
            ctx.emit_durable_event(StreamEventKind::Silence, json!({}))
                .await
        );
        step!(ctx, ctx.emit_done().await);
        return Ok(TurnOutcome::Silence);
    }
    step!(ctx, ctx.emit_done().await);
    Ok(TurnOutcome::Completed)
}

async fn run_resume_inner(
    services: &RuntimeServices,
    req: &ResumeRequest,
    ctx: &mut StreamCtx,
) -> Result<TurnOutcome, Cancelled> {
    if let Err(err) = ctx
        .allocator
        .set_thread_status(&ctx.thread_id, "running")
        .await
    {
        return ctx.fail(&err.to_string()).await;
    }

    let agent = match load_resume_candidate(&services.pool, &req.group_id, &req.agent_id).await {
        Ok(agent) => agent,
        Err(err) => return fail_resume(ctx, &err.to_string()).await,
    };

    if ctx
        .emit(
            StreamEventKind::AgentStart,
            json!({ "agent_id": agent.agent_id, "display_name": agent.display_name }),
        )
        .await
        .is_err()
    {
        let _ = ctx
            .allocator
            .set_thread_status(&ctx.thread_id, "paused")
            .await;
        return Ok(TurnOutcome::Cancelled);
    }

    let provider_cfg = match resolve_provider(&services.pool, &agent).await {
        Ok(config) => config,
        Err(err) => return fail_resume(ctx, &err.to_string()).await,
    };
    let provider = match build_provider(&provider_cfg) {
        Ok(provider) => provider,
        Err(err) => return fail_resume(ctx, &err.to_string()).await,
    };
    let model = model_from_config(&agent.model_config_json, &provider_cfg.default_model);
    let messages = match build_resume_messages(
        &services.pool,
        &ctx.thread_id,
        &agent.system_prompt,
        &req.message_id,
    )
    .await
    {
        Ok(messages) => messages,
        Err(err) => return fail_resume(ctx, &err.to_string()).await,
    };
    let request = ChatRequest {
        model,
        messages,
        temperature: None,
        reasoning_passback: provider_cfg.reasoning_passback,
        tools: Vec::new(),
    };
    let mut deltas = match provider.stream(request).await {
        Ok(deltas) => deltas,
        Err(err) => return fail_resume(ctx, &err.to_string()).await,
    };

    let mut addition = String::new();
    while let Some(delta) = deltas.recv().await {
        match delta {
            ChatDelta::Token(text) => {
                match ctx
                    .emit(
                        StreamEventKind::Token,
                        json!({ "agent_id": agent.agent_id, "text": text, "delta": text }),
                    )
                    .await
                {
                    Ok(()) => addition.push_str(&text),
                    Err(StepErr::Cancelled) => {
                        append_resume_cancellation(ctx, req, &addition).await?;
                        return Ok(TurnOutcome::Cancelled);
                    }
                    Err(StepErr::Db(err)) => return fail_resume(ctx, &err.to_string()).await,
                }
            }
            ChatDelta::Reasoning(text) => {
                if let Err(err) = ctx
                    .emit(
                        StreamEventKind::Reasoning,
                        json!({ "agent_id": agent.agent_id, "text": text, "delta": text }),
                    )
                    .await
                {
                    return match err {
                        StepErr::Cancelled => {
                            append_resume_cancellation(ctx, req, &addition).await?;
                            Ok(TurnOutcome::Cancelled)
                        }
                        StepErr::Db(err) => fail_resume(ctx, &err.to_string()).await,
                    };
                }
            }
            ChatDelta::Usage(usage) => {
                if let Err(err) = ctx
                    .emit(
                        StreamEventKind::ContextUsage,
                        json!({
                            "agent_id": agent.agent_id,
                            "input_tokens": usage.input_tokens,
                            "output_tokens": usage.output_tokens,
                            "total_tokens": usage.total_tokens,
                        }),
                    )
                    .await
                {
                    return match err {
                        StepErr::Cancelled => {
                            append_resume_cancellation(ctx, req, &addition).await?;
                            Ok(TurnOutcome::Cancelled)
                        }
                        StepErr::Db(err) => fail_resume(ctx, &err.to_string()).await,
                    };
                }
            }
            ChatDelta::ToolCall(_) => {}
            ChatDelta::Done => break,
        }
    }

    let final_content = format!("{}{}", req.existing_content, addition);
    let message_payload = json!({
        "message_id": req.message_id,
        "agent_id": agent.agent_id,
        "sender_id": agent.agent_id,
        "display_name": agent.display_name,
        "content": final_content,
    });
    match ctx
        .emit_resume_completion(message_payload, &req.message_id, &final_content)
        .await
    {
        Ok(()) => Ok(TurnOutcome::Completed),
        Err(err) => match err {
            StepErr::Cancelled => {
                append_resume_cancellation(ctx, req, &addition).await?;
                Ok(TurnOutcome::Cancelled)
            }
            StepErr::Db(err) => fail_resume(ctx, &err.to_string()).await,
        },
    }
}

async fn fail_resume(ctx: &mut StreamCtx, message: &str) -> Result<TurnOutcome, Cancelled> {
    let _ = ctx
        .allocator
        .set_thread_status(&ctx.thread_id, "failed")
        .await;
    ctx.fail(message).await
}

async fn append_resume_cancellation(
    ctx: &mut StreamCtx,
    req: &ResumeRequest,
    addition: &str,
) -> Result<(), Cancelled> {
    if let Err(err) = ctx
        .allocator
        .append_interrupted_message(&req.thread_id, &req.message_id, addition, "paused")
        .await
    {
        return fail_resume(ctx, &err.to_string()).await.map(|_| ());
    }
    Ok(())
}

/// An active agent eligible to respond in the group.
struct Candidate {
    agent_id: String,
    owner_id: String,
    display_name: String,
    system_prompt: String,
    runtime_kind: String,
    provider_id: Option<String>,
    model_config_json: Option<String>,
    tool_config_json: Option<String>,
    external_runtime_json: Option<String>,
    skill_ids_json: Option<String>,
    workspace_id: Option<String>,
    share_group_workspace: bool,
    response_mode: String,
    topology_role: Option<String>,
    speaking_order: Option<i64>,
}

struct GroupRuntimeConfig {
    id: String,
    owner_id: String,
    name: String,
    description: Option<String>,
    announcement: Option<String>,
    workspace_id: Option<String>,
    free_speech: bool,
    proactive_mode: bool,
    proactive_reply_multiplier: i64,
    allow_agent_free_mention: bool,
    communication_mode: String,
    muted_agent_ids: HashSet<String>,
}

struct InvocationContext {
    system_prompt: String,
    tools: Vec<ToolDefinition>,
    executor: ToolExecutor,
    workspace_root: Option<PathBuf>,
}

/// Resolved LLM provider connection settings.
struct ProviderConfig {
    kind: String,
    base_url: Option<String>,
    api_key: String,
    default_model: String,
    reasoning_passback: bool,
}

enum AgentRunResult {
    NoVisible,
    Visible,
    WaitingForUser,
    Handoff { helper: Box<Candidate> },
}

async fn run_agent_turn(
    services: &RuntimeServices,
    ctx: &mut StreamCtx,
    agent: &Candidate,
    group: &GroupRuntimeConfig,
    handoff_depth: usize,
) -> Result<AgentRunResult, StepErr> {
    ctx.emit(
        StreamEventKind::AgentStart,
        json!({ "agent_id": agent.agent_id, "display_name": agent.display_name }),
    )
    .await?;

    if agent.runtime_kind == "acp" {
        return run_acp_agent_turn(services, ctx, agent, group).await;
    }

    let provider_cfg = resolve_provider(&services.pool, agent)
        .await
        .map_err(StepErr::Db)?;
    let provider = build_provider(&provider_cfg).map_err(StepErr::Db)?;
    let model = model_from_config(&agent.model_config_json, &provider_cfg.default_model);
    let invocation = build_invocation_context(&services.pool, ctx, agent, group)
        .await
        .map_err(StepErr::Db)?;
    let mut messages = build_messages(&services.pool, &ctx.thread_id, &invocation.system_prompt)
        .await
        .map_err(StepErr::Db)?;

    let mut content = String::new();
    let checkpoint_interrupted = handoff_depth == 0;

    for _ in 0..MAX_TOOL_ROUNDS {
        let request = ChatRequest {
            model: model.clone(),
            messages: messages.clone(),
            temperature: None,
            reasoning_passback: provider_cfg.reasoning_passback,
            tools: invocation.tools.clone(),
        };
        let mut deltas = provider.stream(request).await.map_err(StepErr::Db)?;
        let mut round_content = String::new();
        let mut tool_calls = Vec::new();

        while let Some(delta) = deltas.recv().await {
            match delta {
                ChatDelta::Token(text) => {
                    match ctx
                        .emit(
                            StreamEventKind::Token,
                            json!({ "agent_id": agent.agent_id, "text": text, "delta": text }),
                        )
                        .await
                    {
                        Ok(()) => {
                            content.push_str(&text);
                            round_content.push_str(&text);
                        }
                        Err(StepErr::Cancelled) => {
                            maybe_persist_interrupted_agent(
                                ctx,
                                agent,
                                &content,
                                checkpoint_interrupted,
                            )
                            .await?;
                            return Err(StepErr::Cancelled);
                        }
                        Err(err @ StepErr::Db(_)) => return Err(err),
                    }
                }
                ChatDelta::Reasoning(text) => {
                    if let Err(err) = ctx
                        .emit(
                            StreamEventKind::Reasoning,
                            json!({ "agent_id": agent.agent_id, "text": text, "delta": text }),
                        )
                        .await
                    {
                        if matches!(err, StepErr::Cancelled) {
                            maybe_persist_interrupted_agent(
                                ctx,
                                agent,
                                &content,
                                checkpoint_interrupted,
                            )
                            .await?;
                        }
                        return Err(err);
                    }
                }
                ChatDelta::ToolCall(call) => {
                    tool_calls.push(call);
                }
                ChatDelta::Usage(usage) => {
                    if let Err(err) = ctx
                        .emit(
                            StreamEventKind::ContextUsage,
                            json!({
                                "agent_id": agent.agent_id,
                                "input_tokens": usage.input_tokens,
                                "output_tokens": usage.output_tokens,
                                "total_tokens": usage.total_tokens,
                            }),
                        )
                        .await
                    {
                        if matches!(err, StepErr::Cancelled) {
                            maybe_persist_interrupted_agent(
                                ctx,
                                agent,
                                &content,
                                checkpoint_interrupted,
                            )
                            .await?;
                        }
                        return Err(err);
                    }
                }
                ChatDelta::Done => break,
            }
        }

        if tool_calls.is_empty() {
            return finish_agent_content(
                ctx,
                agent,
                group.proactive_mode,
                content,
                checkpoint_interrupted,
            )
            .await;
        }

        if let Some(call) = agent_as_tool_call(&tool_calls) {
            return handle_agent_as_tool(
                services,
                ctx,
                agent,
                group,
                handoff_depth,
                call,
                &content,
            )
            .await;
        }

        messages.push(ChatMessage::assistant_tool_calls(
            round_content,
            tool_calls.clone(),
        ));

        let mut wait_for_user: Option<Value> = None;
        for call in tool_calls {
            let result = execute_tool_call(
                ctx,
                agent,
                &invocation.executor,
                &call,
                checkpoint_interrupted,
                &content,
            )
            .await?;
            messages.push(ChatMessage::tool_result(
                call.id,
                call.name,
                format!("status: {:?}\n{}", result.status, result.output),
            ));
            if matches!(result.status, ToolStatus::WaitingForUser) {
                wait_for_user = Some(tool_input_request_payload(&result.output));
                break;
            }
        }

        if let Some(input_request) = wait_for_user {
            ctx.emit_durable_event(
                StreamEventKind::WaitingForUser,
                json!({
                    "agent_id": agent.agent_id,
                    "message": "Waiting for your input",
                    "input_request": input_request,
                }),
            )
            .await?;
            return Ok(AgentRunResult::WaitingForUser);
        }
    }

    content.push_str("\n\nTool loop stopped after repeated tool calls without a final answer.");
    finish_agent_content(
        ctx,
        agent,
        group.proactive_mode,
        content,
        checkpoint_interrupted,
    )
    .await
}

async fn maybe_persist_interrupted_agent(
    ctx: &mut StreamCtx,
    agent: &Candidate,
    content: &str,
    checkpoint_interrupted: bool,
) -> Result<(), StepErr> {
    if checkpoint_interrupted {
        persist_interrupted_agent(ctx, agent, content).await?;
    }
    Ok(())
}

async fn run_acp_agent_turn(
    services: &RuntimeServices,
    ctx: &mut StreamCtx,
    agent: &Candidate,
    group: &GroupRuntimeConfig,
) -> Result<AgentRunResult, StepErr> {
    let raw = agent
        .external_runtime_json
        .as_deref()
        .and_then(|value| serde_json::from_str::<Value>(value).ok());
    let config = normalize_acp_runtime(raw.as_ref()).map_err(|err| StepErr::Db(err.into()))?;
    let invocation = build_invocation_context(&services.pool, ctx, agent, group)
        .await
        .map_err(StepErr::Db)?;
    let cwd = invocation.workspace_root.ok_or_else(|| {
        StepErr::Db(anyhow::anyhow!(
            "ACP agent requires an active local workspace context"
        ))
    })?;
    let prompt = build_acp_prompt(&services.pool, &ctx.thread_id, &invocation.system_prompt)
        .await
        .map_err(StepErr::Db)?;

    let mut run = run_acp_agent_stream(
        services.pool.clone(),
        AcpRunRequest {
            owner_id: agent.owner_id.clone(),
            group_id: Some(ctx.group_id.clone()),
            agent_id: agent.agent_id.clone(),
            thread_id: Some(ctx.thread_id.clone()),
            config,
            cwd,
            prompt,
        },
    )
    .await
    .map_err(|err| StepErr::Db(err.into()))?;

    let mut content = String::new();
    while let Some(event) = run.next_event().await {
        match event.kind {
            AcpEventKind::Run => {
                ctx.emit(StreamEventKind::AcpAgentRun, event.data).await?;
            }
            AcpEventKind::Token => {
                let text = event.data.as_str().unwrap_or_default().to_string();
                if !text.is_empty() {
                    ctx.emit(
                        StreamEventKind::Token,
                        json!({ "agent_id": agent.agent_id, "text": text, "delta": text }),
                    )
                    .await?;
                    content.push_str(&text);
                }
            }
            AcpEventKind::Reasoning => {
                let text = event.data.as_str().unwrap_or_default().to_string();
                if !text.is_empty() {
                    ctx.emit(
                        StreamEventKind::Reasoning,
                        json!({ "agent_id": agent.agent_id, "text": text, "delta": text }),
                    )
                    .await?;
                }
            }
            AcpEventKind::ToolCallStart => {
                ctx.emit(StreamEventKind::ToolCallStart, event.data).await?;
            }
            AcpEventKind::ToolCallResult => {
                ctx.emit(StreamEventKind::ToolCallResult, event.data)
                    .await?;
            }
            AcpEventKind::Usage => {
                ctx.emit(
                    StreamEventKind::ContextUsage,
                    json!({ "agent_id": agent.agent_id, "usage": event.data }),
                )
                .await?;
            }
        }
    }
    run.join().await.map_err(|err| StepErr::Db(err.into()))?;

    finish_agent_content(ctx, agent, group.proactive_mode, content, true).await
}

async fn execute_tool_call(
    ctx: &mut StreamCtx,
    agent: &Candidate,
    executor: &ToolExecutor,
    call: &ToolCall,
    checkpoint_interrupted: bool,
    content: &str,
) -> Result<ToolResult, StepErr> {
    if let Err(err) = emit_tool_call_start(ctx, agent, call).await {
        if matches!(err, StepErr::Cancelled) {
            maybe_persist_interrupted_agent(ctx, agent, content, checkpoint_interrupted).await?;
        }
        return Err(err);
    }

    let result = executor.execute(&call.name, call.args.clone()).await;
    if let Err(err) = ctx
        .emit(
            StreamEventKind::ToolCallResult,
            json!({
                "agent_id": agent.agent_id,
                "display_name": agent.display_name,
                "tool_call_id": call.id,
                "tool_name": call.name,
                "status": tool_status_wire(result.status),
                "result_summary": summarize_text(&result.output),
                "output": result.output,
            }),
        )
        .await
    {
        if matches!(err, StepErr::Cancelled) {
            maybe_persist_interrupted_agent(ctx, agent, content, checkpoint_interrupted).await?;
        }
        return Err(err);
    }
    Ok(result)
}

fn tool_status_wire(status: ToolStatus) -> &'static str {
    match status {
        ToolStatus::Completed => "completed",
        ToolStatus::SetupRequired => "setup_required",
        ToolStatus::WorkspaceRequired => "workspace_required",
        ToolStatus::WaitingForUser => "input_required",
        ToolStatus::InputRequested => "input_required",
        ToolStatus::ApprovalRequired => "approval_required",
        ToolStatus::Failed => "failed",
    }
}

fn summarize_text(value: &str) -> String {
    const LIMIT: usize = 500;
    let mut chars = value.chars();
    let summary: String = chars.by_ref().take(LIMIT).collect();
    if chars.next().is_some() {
        format!("{summary}...")
    } else {
        value.to_string()
    }
}

fn tool_input_request_payload(output: &str) -> Value {
    serde_json::from_str::<Value>(output)
        .ok()
        .and_then(|value| value.get("input_request").cloned())
        .unwrap_or_else(|| json!({ "question": "The agent requested input.", "required": true }))
}

async fn persist_interrupted_agent(
    ctx: &mut StreamCtx,
    agent: &Candidate,
    content: &str,
) -> Result<(), StepErr> {
    let Some(content) = interrupted_visible_content(content) else {
        return Ok(());
    };
    let message = NewMessage {
        id: Uuid::new_v4().to_string(),
        sender_type: "agent".to_string(),
        sender_id: Some(agent.agent_id.clone()),
        message_type: "text".to_string(),
        content,
    };
    ctx.allocator
        .persist_interrupted_message(&ctx.thread_id, &ctx.group_id, &message)
        .await
        .map_err(StepErr::Db)?;
    Ok(())
}

fn interrupted_visible_content(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() || trimmed == SILENT_MARKER {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix(WAITING_MARKER) {
        let rest = rest.trim();
        if rest.is_empty() {
            return None;
        }
        return Some(rest.to_string());
    }
    Some(content.to_string())
}

async fn handle_agent_as_tool(
    services: &RuntimeServices,
    ctx: &mut StreamCtx,
    agent: &Candidate,
    group: &GroupRuntimeConfig,
    handoff_depth: usize,
    call: ToolCall,
    content: &str,
) -> Result<AgentRunResult, StepErr> {
    emit_tool_call_start(ctx, agent, &call).await?;

    let parsed = match AgentAsToolCall::from_args(call.id.clone(), &call.args) {
        Ok(parsed) => parsed,
        Err(failure) => {
            emit_tool_call_failure(ctx, agent, &call.id, &failure).await?;
            return finish_agent_content(
                ctx,
                agent,
                false,
                content.to_string(),
                handoff_depth == 0,
            )
            .await;
        }
    };
    let caller = CallerAgent {
        agent_id: agent.agent_id.clone(),
        owner_id: agent.owner_id.clone(),
        display_name: agent.display_name.clone(),
        tool_config_json: agent.tool_config_json.clone(),
    };

    let dispatch = match resolve_dispatch(
        &services.pool,
        &ctx.group_id,
        &caller,
        &parsed,
        handoff_depth,
        &group.muted_agent_ids,
    )
    .await
    {
        Ok(dispatch) => dispatch,
        Err(failure) => {
            emit_tool_call_failure(ctx, agent, &parsed.tool_call_id, &failure).await?;
            return finish_agent_content(
                ctx,
                agent,
                false,
                content.to_string(),
                handoff_depth == 0,
            )
            .await;
        }
    };
    let helper_candidate = match load_candidate_by_id(
        &services.pool,
        &ctx.group_id,
        &dispatch.helper.agent_id,
        group,
    )
    .await
    {
        Ok(candidate) => candidate,
        Err(err) => {
            let failure = AgentAsToolFailure::unavailable(err.to_string());
            emit_tool_call_failure(ctx, agent, &parsed.tool_call_id, &failure).await?;
            return finish_agent_content(
                ctx,
                agent,
                false,
                content.to_string(),
                handoff_depth == 0,
            )
            .await;
        }
    };

    let agent_message = NewMessage {
        id: Uuid::new_v4().to_string(),
        sender_type: "agent".to_string(),
        sender_id: Some(agent.agent_id.clone()),
        message_type: "text".to_string(),
        content: dispatch.content.clone(),
    };
    let message_payload = json!({
        "message_id": agent_message.id,
        "agent_id": agent.agent_id,
        "sender_id": agent.agent_id,
        "display_name": agent.display_name,
        "content": dispatch.content,
        "dispatch": true,
    });
    ctx.emit_message(
        StreamEventKind::AgentMessage,
        message_payload,
        &agent_message,
    )
    .await?;

    ctx.emit(
        StreamEventKind::ToolCallResult,
        json!({
            "agent_id": agent.agent_id,
            "display_name": agent.display_name,
            "tool_call_id": parsed.tool_call_id,
            "tool_name": AGENT_AS_TOOL_NAME,
            "status": "completed",
            "result_summary": format!(
                "Dispatched to @{} through normal group routing.",
                dispatch.helper.display_name
            ),
        }),
    )
    .await?;

    Ok(AgentRunResult::Handoff {
        helper: Box::new(helper_candidate),
    })
}

fn agent_as_tool_call(tool_calls: &[ToolCall]) -> Option<ToolCall> {
    tool_calls
        .iter()
        .find(|call| call.name == AGENT_AS_TOOL_NAME)
        .cloned()
}

async fn finish_agent_content(
    ctx: &mut StreamCtx,
    agent: &Candidate,
    proactive: bool,
    content: String,
    checkpoint_interrupted: bool,
) -> Result<AgentRunResult, StepErr> {
    let trimmed = content.trim();

    if proactive && trimmed == SILENT_MARKER {
        ctx.emit_durable_event(
            StreamEventKind::AgentSilent,
            json!({ "agent_id": agent.agent_id, "display_name": agent.display_name }),
        )
        .await?;
        return Ok(AgentRunResult::NoVisible);
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
        content
    };

    if visible.trim().is_empty() {
        return Ok(AgentRunResult::NoVisible);
    }

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
        "sender_id": agent.agent_id,
        "display_name": agent.display_name,
        "content": visible.clone(),
    });
    if let Err(err) = ctx
        .emit_message(
            StreamEventKind::AgentMessage,
            message_payload,
            &agent_message,
        )
        .await
    {
        if matches!(err, StepErr::Cancelled) {
            maybe_persist_interrupted_agent(ctx, agent, &visible, checkpoint_interrupted).await?;
        }
        return Err(err);
    }

    if is_waiting {
        ctx.emit_durable_event(
            StreamEventKind::WaitingForUser,
            json!({ "agent_id": agent.agent_id, "message": visible }),
        )
        .await?;
        Ok(AgentRunResult::WaitingForUser)
    } else {
        Ok(AgentRunResult::Visible)
    }
}

async fn emit_tool_call_start(
    ctx: &mut StreamCtx,
    agent: &Candidate,
    call: &ToolCall,
) -> Result<(), StepErr> {
    ctx.emit(
        StreamEventKind::ToolCallStart,
        json!({
            "agent_id": agent.agent_id,
            "display_name": agent.display_name,
            "tool_call_id": call.id,
            "tool_name": call.name,
            "status": "started",
            "args_summary": summarize_value(&call.args),
            "args": call.args,
        }),
    )
    .await
}

async fn emit_tool_call_failure(
    ctx: &mut StreamCtx,
    agent: &Candidate,
    tool_call_id: &str,
    failure: &AgentAsToolFailure,
) -> Result<(), StepErr> {
    ctx.emit(
        StreamEventKind::ToolCallResult,
        json!({
            "agent_id": agent.agent_id,
            "display_name": agent.display_name,
            "tool_call_id": tool_call_id,
            "tool_name": AGENT_AS_TOOL_NAME,
            "status": failure.status,
            "result_summary": failure.message,
        }),
    )
    .await
}

fn summarize_value(value: &Value) -> String {
    let raw = value.to_string();
    const LIMIT: usize = 240;
    let mut chars = raw.chars();
    let summary: String = chars.by_ref().take(LIMIT).collect();
    if chars.next().is_some() {
        format!("{summary}...")
    } else {
        raw
    }
}

#[derive(sqlx::FromRow)]
struct GroupRuntimeRow {
    id: String,
    owner_id: String,
    name: String,
    description: Option<String>,
    announcement: Option<String>,
    workspace_id: Option<String>,
    free_speech: i64,
    proactive_mode: i64,
    proactive_reply_multiplier: i64,
    allow_agent_free_mention: i64,
    communication_mode: String,
    muted_agent_ids_json: Option<String>,
}

async fn load_group_runtime_config(
    pool: &SqlitePool,
    group_id: &str,
) -> anyhow::Result<GroupRuntimeConfig> {
    let row: Option<GroupRuntimeRow> = sqlx::query_as(
        "SELECT id, owner_id, name, description, announcement, workspace_id, free_speech, \
                proactive_mode, proactive_reply_multiplier, allow_agent_free_mention, \
                communication_mode, muted_agent_ids_json \
         FROM groups WHERE id = ? AND status = 'active'",
    )
    .bind(group_id)
    .fetch_optional(pool)
    .await?;
    let row = row.ok_or_else(|| anyhow::anyhow!("group not found"))?;
    Ok(GroupRuntimeConfig {
        id: row.id,
        owner_id: row.owner_id,
        name: row.name,
        description: row.description,
        announcement: row.announcement,
        workspace_id: row.workspace_id,
        free_speech: row.free_speech != 0,
        proactive_mode: row.proactive_mode != 0,
        proactive_reply_multiplier: row.proactive_reply_multiplier,
        allow_agent_free_mention: row.allow_agent_free_mention != 0,
        communication_mode: row.communication_mode,
        muted_agent_ids: parse_string_set(row.muted_agent_ids_json.as_deref()),
    })
}

#[derive(sqlx::FromRow)]
struct CandidateRow {
    id: String,
    owner_id: String,
    display_name: Option<String>,
    name: String,
    system_prompt: String,
    runtime_kind: String,
    provider_id: Option<String>,
    model_config_json: Option<String>,
    tool_config_json: Option<String>,
    external_runtime_json: Option<String>,
    skill_ids_json: Option<String>,
    workspace_id: Option<String>,
    context_scope_json: Option<String>,
    response_mode: String,
    topology_role: Option<String>,
    speaking_order: Option<i64>,
}

async fn load_candidates(
    pool: &SqlitePool,
    group_id: &str,
    group: &GroupRuntimeConfig,
) -> anyhow::Result<Vec<Candidate>> {
    let rows: Vec<CandidateRow> = sqlx::query_as(
        "SELECT a.id, a.owner_id, ga.display_name, a.name, a.system_prompt, a.runtime_kind, \
                a.provider_id, a.model_config_json, a.tool_config_json, \
                a.external_runtime_json, a.skill_ids_json, a.workspace_id, \
                ga.context_scope_json, ga.response_mode, ga.topology_role, ga.speaking_order \
         FROM group_agents ga \
         JOIN agents a ON a.id = ga.agent_id \
         WHERE ga.group_id = ? AND ga.status = 'active' AND a.status = 'active' \
         ORDER BY COALESCE(NULLIF(ga.speaking_order, 0), 9223372036854775807) ASC, \
                  ga.joined_at ASC, a.id ASC",
    )
    .bind(group_id)
    .fetch_all(pool)
    .await?;

    // First-match-wins when two agents share an effective display name.
    let mut seen_names: HashSet<String> = HashSet::new();
    let mut candidates = Vec::new();
    for row in rows {
        if group.muted_agent_ids.contains(&row.id) {
            continue;
        }
        let display = row.display_name.clone().unwrap_or_else(|| row.name.clone());
        if !seen_names.insert(display.to_lowercase()) {
            continue;
        }
        candidates.push(candidate_from_row(row));
    }
    Ok(candidates)
}

async fn load_resume_candidate(
    pool: &SqlitePool,
    group_id: &str,
    agent_id: &str,
) -> anyhow::Result<Candidate> {
    let row: Option<CandidateRow> = sqlx::query_as(
        "SELECT a.id, a.owner_id, ga.display_name, a.name, a.system_prompt, a.runtime_kind, \
                a.provider_id, a.model_config_json, a.tool_config_json, \
                a.external_runtime_json, a.skill_ids_json, a.workspace_id, \
                ga.context_scope_json, ga.response_mode, ga.topology_role, ga.speaking_order \
         FROM group_agents ga \
         JOIN agents a ON a.id = ga.agent_id \
         WHERE ga.group_id = ? \
           AND ga.agent_id = ? \
           AND ga.status = 'active' \
           AND a.status = 'active'",
    )
    .bind(group_id)
    .bind(agent_id)
    .fetch_optional(pool)
    .await?;
    row.map(candidate_from_row)
        .ok_or_else(|| anyhow::anyhow!("agent is no longer active in this group"))
}

async fn load_candidate_by_id(
    pool: &SqlitePool,
    group_id: &str,
    agent_id: &str,
    group: &GroupRuntimeConfig,
) -> anyhow::Result<Candidate> {
    if group.muted_agent_ids.contains(agent_id) {
        anyhow::bail!("assistant agent is muted in this group");
    }
    let row: Option<CandidateRow> = sqlx::query_as(
        "SELECT a.id, a.owner_id, ga.display_name, a.name, a.system_prompt, a.runtime_kind, \
                a.provider_id, a.model_config_json, a.tool_config_json, \
                a.external_runtime_json, a.skill_ids_json, a.workspace_id, \
                ga.context_scope_json, ga.response_mode, ga.topology_role, ga.speaking_order \
         FROM group_agents ga \
         JOIN agents a ON a.id = ga.agent_id \
         WHERE ga.group_id = ? \
           AND ga.agent_id = ? \
           AND ga.status = 'active' \
           AND a.status = 'active'",
    )
    .bind(group_id)
    .bind(agent_id)
    .fetch_optional(pool)
    .await?;
    row.map(candidate_from_row)
        .ok_or_else(|| anyhow::anyhow!("assistant agent is no longer active in this group"))
}

fn candidate_from_row(row: CandidateRow) -> Candidate {
    let display = row.display_name.clone().unwrap_or_else(|| row.name.clone());
    Candidate {
        agent_id: row.id,
        owner_id: row.owner_id,
        display_name: display,
        system_prompt: row.system_prompt,
        runtime_kind: row.runtime_kind,
        provider_id: row.provider_id,
        model_config_json: row.model_config_json,
        tool_config_json: row.tool_config_json,
        external_runtime_json: row.external_runtime_json,
        skill_ids_json: row.skill_ids_json,
        workspace_id: row.workspace_id,
        share_group_workspace: group_workspace_shared(row.context_scope_json.as_deref()),
        response_mode: row.response_mode,
        topology_role: row.topology_role,
        speaking_order: row.speaking_order,
    }
}

/// Pick the responders for `text`: explicit mentions win; otherwise free-speech
/// or proactive mode fans out to everyone; otherwise nobody.
fn select_agents(
    candidates: Vec<Candidate>,
    text: &str,
    group: &GroupRuntimeConfig,
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
    if group.free_speech || group.proactive_mode {
        return candidates
            .into_iter()
            .filter(|candidate| {
                candidate.response_mode != "explicit_only"
                    && candidate.response_mode != "muted"
                    && candidate.response_mode != "manual_only"
            })
            .collect();
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

fn parse_string_set(raw: Option<&str>) -> HashSet<String> {
    raw.and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
        .unwrap_or_default()
        .into_iter()
        .collect()
}

fn group_workspace_shared(raw: Option<&str>) -> bool {
    raw.and_then(|value| serde_json::from_str::<Value>(value).ok())
        .and_then(|value| {
            value
                .get("share_group_workspace")
                .and_then(Value::as_bool)
                .or(Some(false))
        })
        .unwrap_or(false)
}

async fn build_invocation_context(
    pool: &SqlitePool,
    ctx: &StreamCtx,
    agent: &Candidate,
    group: &GroupRuntimeConfig,
) -> anyhow::Result<InvocationContext> {
    let enabled_tools = enabled_tool_names(agent.tool_config_json.as_deref());
    let mounted_skills = load_mounted_skills(pool, agent).await?;
    let workspace_root = resolve_workspace_root(pool, agent, group).await?;
    let executor = ToolExecutor::new_with_skills(workspace_root.clone(), mounted_skills.clone())
        .map_err(|err| anyhow::anyhow!(err.model_safe_message()))?;
    let tools = enabled_tools
        .iter()
        .filter_map(|name| tool_definition(name))
        .collect::<Vec<_>>();
    let system_prompt = build_agent_system_prompt(
        pool,
        ctx,
        agent,
        group,
        &enabled_tools,
        &mounted_skills,
        &workspace_root,
    )
    .await?;

    Ok(InvocationContext {
        system_prompt,
        tools,
        executor,
        workspace_root,
    })
}

async fn build_agent_system_prompt(
    pool: &SqlitePool,
    ctx: &StreamCtx,
    agent: &Candidate,
    group: &GroupRuntimeConfig,
    enabled_tools: &[String],
    mounted_skills: &[MountedSkill],
    workspace_root: &Option<PathBuf>,
) -> anyhow::Result<String> {
    let roster = load_group_roster(pool, &ctx.group_id, &agent.agent_id).await?;
    let skill_lines = if mounted_skills.is_empty() {
        "none".to_string()
    } else {
        mounted_skills
            .iter()
            .map(|skill| {
                format!(
                    "- {}{}",
                    skill.name,
                    skill
                        .description
                        .as_ref()
                        .map(|description| format!(": {description}"))
                        .unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let tools = if enabled_tools.is_empty() {
        "none".to_string()
    } else {
        enabled_tools.join(", ")
    };
    let workspace_source = if workspace_root.is_some() {
        if agent.share_group_workspace {
            "group"
        } else {
            "agent"
        }
    } else {
        "none"
    };
    let workspace_location = workspace_root
        .as_ref()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| "not configured".to_string());
    let mut sections = vec![
        agent.system_prompt.clone(),
        format!(
            "Group context:\n- id: {}\n- owner_id: {}\n- name: {}\n- description: {}\n- announcement: {}\n- communication_mode: {}\n- proactive_reply_multiplier: {}\n- allow_agent_free_mention: {}\n- self_display_name: {}\n- self_response_mode: {}\n- self_topology_role: {}\n- self_speaking_order: {}",
            group.id,
            group.owner_id,
            group.name,
            group.description.as_deref().unwrap_or("none"),
            group.announcement.as_deref().unwrap_or("none"),
            group.communication_mode,
            group.proactive_reply_multiplier,
            group.allow_agent_free_mention,
            agent.display_name,
            agent.response_mode,
            agent.topology_role.as_deref().unwrap_or("none"),
            agent
                .speaking_order
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
        ),
        format!("Roster:\n{roster}"),
        format!(
            "Workspace:\n- source: {workspace_source}\n- location: {workspace_location}"
        ),
        format!("Enabled provider-native tools: {tools}"),
        format!("Mounted skills:\n{skill_lines}"),
        "Only provider-native tool calls listed above may execute. Literal XML or pseudo-tool text is not executable tool work.".to_string(),
    ];
    if group.proactive_mode {
        sections.push(format!(
            "Proactive mode is enabled. Reply with exactly {SILENT_MARKER} to skip this turn without persisting a message."
        ));
    }
    Ok(sections.join("\n\n"))
}

async fn load_group_roster(
    pool: &SqlitePool,
    group_id: &str,
    self_agent_id: &str,
) -> anyhow::Result<String> {
    let agents: Vec<(String, Option<String>, String)> = sqlx::query_as(
        "SELECT a.id, ga.display_name, a.name \
         FROM group_agents ga JOIN agents a ON a.id = ga.agent_id \
         WHERE ga.group_id = ? AND ga.status = 'active' AND a.status = 'active' \
         ORDER BY ga.joined_at ASC, a.id ASC",
    )
    .bind(group_id)
    .fetch_all(pool)
    .await?;
    let humans: Vec<(String, String)> = sqlx::query_as(
        "SELECT u.name, gm.role \
         FROM group_members gm JOIN users u ON u.id = gm.user_id \
         WHERE gm.group_id = ? AND gm.status = 'active' \
         ORDER BY gm.joined_at ASC, u.id ASC",
    )
    .bind(group_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut lines = Vec::new();
    for (id, display_name, name) in agents {
        let display = display_name.unwrap_or(name);
        let marker = if id == self_agent_id { " (self)" } else { "" };
        lines.push(format!("- agent: {display}{marker}"));
    }
    for (name, role) in humans {
        lines.push(format!("- human: {name} ({role})"));
    }
    if lines.is_empty() {
        Ok("none".to_string())
    } else {
        Ok(lines.join("\n"))
    }
}

async fn resolve_workspace_root(
    pool: &SqlitePool,
    agent: &Candidate,
    group: &GroupRuntimeConfig,
) -> anyhow::Result<Option<PathBuf>> {
    let workspace_id = if agent.share_group_workspace {
        group.workspace_id.as_deref()
    } else {
        agent.workspace_id.as_deref()
    };
    let Some(workspace_id) = workspace_id else {
        return Ok(None);
    };
    let row: Option<(String, Option<String>, String)> = sqlx::query_as(
        "SELECT backend_type, local_path, status FROM workspaces WHERE id = ? AND owner_id = ?",
    )
    .bind(workspace_id)
    .bind(&agent.owner_id)
    .fetch_optional(pool)
    .await?;
    let Some((backend_type, local_path, status)) = row else {
        return Ok(None);
    };
    if status != "active" || backend_type != "local" {
        return Ok(None);
    }
    Ok(local_path.map(PathBuf::from))
}

async fn load_mounted_skills(
    pool: &SqlitePool,
    agent: &Candidate,
) -> anyhow::Result<Vec<MountedSkill>> {
    let ids = agent
        .skill_ids_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok())
        .unwrap_or_default();
    let mut skills = Vec::new();
    for id in ids {
        let row: Option<(String, Option<String>, Option<String>, String)> = sqlx::query_as(
            "SELECT name, description, metadata_json, body_markdown \
             FROM skills WHERE id = ? AND owner_id = ? AND status = 'active'",
        )
        .bind(&id)
        .bind(&agent.owner_id)
        .fetch_optional(pool)
        .await?;
        if let Some((name, description, metadata_json, body_markdown)) = row {
            skills.push(MountedSkill {
                name,
                description,
                metadata: metadata_json
                    .and_then(|raw| serde_json::from_str(&raw).ok())
                    .unwrap_or(Value::Null),
                body_markdown,
            });
        }
    }
    Ok(skills)
}

fn enabled_tool_names(raw: Option<&str>) -> Vec<String> {
    let Some(value) = raw.and_then(|raw| serde_json::from_str::<Value>(raw).ok()) else {
        return vec!["Read".to_string(), "Glob".to_string(), "Grep".to_string()];
    };
    let mut names = Vec::new();
    if let Some(tools) = value.get("tools").and_then(Value::as_object) {
        for (id, selection) in tools {
            if selection
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                if let Some(name) = builtin_tool_name(id) {
                    names.push(name.to_string());
                }
            }
        }
    }
    if let Some(enabled) = value.get("enabled").and_then(Value::as_array) {
        for id in enabled.iter().filter_map(Value::as_str) {
            if let Some(name) = builtin_tool_name(id) {
                names.push(name.to_string());
            }
        }
    }
    if value
        .get("assistant_agents")
        .and_then(Value::as_array)
        .is_some_and(|agents| {
            agents.iter().any(|entry| {
                entry
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true)
            })
        })
    {
        names.push(AGENT_AS_TOOL_NAME.to_string());
    }
    names.sort();
    names.dedup();
    names
}

fn builtin_tool_name(id: &str) -> Option<&'static str> {
    match id {
        "read" => Some("Read"),
        "write" => Some("Write"),
        "edit" => Some("Edit"),
        "glob" => Some("Glob"),
        "grep" => Some("Grep"),
        "bash" => Some("Bash"),
        "ask_user" => Some("AskUser"),
        "web_search" => Some("WebSearch"),
        "fetch" => Some("Fetch"),
        "run_sub_agent" => Some("RunSubAgent"),
        "generate_image" => Some("GenerateImage"),
        "generate_video" => Some("GenerateVideo"),
        "skill_manager" => Some("SkillManager"),
        "todo_write" => Some("TodoWrite"),
        "exit_plan_mode" => Some("ExitPlanMode"),
        _ => None,
    }
}

fn tool_definition(name: &str) -> Option<ToolDefinition> {
    let (description, schema) = match name {
        "Read" => (
            "Read a UTF-8 file from the bound workspace.",
            object_schema(
                &[
                    ("file_path", "string"),
                    ("start_line", "integer"),
                    ("limit", "integer"),
                ],
                &["file_path"],
            ),
        ),
        "Write" => (
            "Create or replace a UTF-8 file in the bound workspace.",
            object_schema(
                &[("file_path", "string"), ("content", "string")],
                &["file_path", "content"],
            ),
        ),
        "Edit" => (
            "Replace exact text in a UTF-8 workspace file.",
            object_schema(
                &[
                    ("file_path", "string"),
                    ("old_string", "string"),
                    ("new_string", "string"),
                    ("replace_all", "boolean"),
                ],
                &["file_path", "old_string", "new_string"],
            ),
        ),
        "Glob" => (
            "Find workspace files by glob pattern.",
            object_schema(&[("pattern", "string"), ("limit", "integer")], &[]),
        ),
        "Grep" => (
            "Search workspace file contents.",
            object_schema(
                &[
                    ("pattern", "string"),
                    ("path", "string"),
                    ("limit", "integer"),
                ],
                &["pattern"],
            ),
        ),
        "Bash" => (
            "Run a guarded shell command in the bound workspace.",
            object_schema(
                &[("command", "string"), ("timeout_seconds", "integer")],
                &["command"],
            ),
        ),
        "AskUser" => (
            "Ask the human user for bounded input without blocking the server.",
            object_schema(
                &[
                    ("question", "string"),
                    ("required", "boolean"),
                    ("choices", "array"),
                ],
                &["question"],
            ),
        ),
        "WebSearch" => (
            "Search the web when a provider is configured; otherwise report setup required.",
            object_schema(
                &[("query", "string"), ("max_results", "integer")],
                &["query"],
            ),
        ),
        "Fetch" => (
            "Fetch a bounded HTTP(S) text URL.",
            object_schema(
                &[("url", "string"), ("timeout_seconds", "integer")],
                &["url"],
            ),
        ),
        "RunSubAgent" => (
            "Request a bounded sub-agent delegation if infrastructure is configured.",
            object_schema(&[("task", "string")], &["task"]),
        ),
        "GenerateImage" | "GenerateVideo" => (
            "Request media generation if infrastructure is configured.",
            object_schema(&[("prompt", "string")], &["prompt"]),
        ),
        "SkillManager" => (
            "List or inspect mounted skill metadata and instructions.",
            object_schema(&[("action", "string"), ("skill_name", "string")], &[]),
        ),
        "TodoWrite" => (
            "Record bounded todo items for this turn.",
            object_schema(&[("todos", "array")], &[]),
        ),
        "ExitPlanMode" => (
            "Request user approval for an implementation plan.",
            object_schema(&[("plan", "string")], &["plan"]),
        ),
        AGENT_AS_TOOL_NAME => (
            "Dispatch a task to a bound assistant that is active in this group.",
            object_schema(
                &[
                    ("assistant", "string"),
                    ("task", "string"),
                    ("instructions", "string"),
                ],
                &["assistant", "task"],
            ),
        ),
        _ => return None,
    };
    Some(ToolDefinition {
        name: name.to_string(),
        description: description.to_string(),
        input_schema: schema,
    })
}

fn object_schema(fields: &[(&str, &str)], required: &[&str]) -> Value {
    let mut properties = serde_json::Map::new();
    for (name, kind) in fields {
        let schema = if *kind == "array" {
            json!({ "type": "array", "items": { "type": "string" } })
        } else {
            json!({ "type": kind })
        };
        properties.insert((*name).to_string(), schema);
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

async fn resolve_provider(pool: &SqlitePool, agent: &Candidate) -> anyhow::Result<ProviderConfig> {
    let provider_id = agent
        .provider_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("agent has no llm provider configured"))?;
    let row: Option<(String, Option<String>, String, String, i64)> = sqlx::query_as(
        "SELECT kind, base_url, api_key, default_model, reasoning_passback \
         FROM llm_providers WHERE id = ? AND owner_id = ? AND status = 'active'",
    )
    .bind(provider_id)
    .bind(&agent.owner_id)
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
        "openai-compatible" | "openai_compatible" | "openai" | "deepseek" | "vllm"
        | "openrouter" => Box::new(OpenAiCompatibleProvider::new(base_url, cfg.api_key.clone())),
        "anthropic" | "anthropic-compatible" | "anthropic_compatible" => {
            Box::new(AnthropicProvider::new(base_url, cfg.api_key.clone()))
        }
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

    let mut messages = vec![ChatMessage::text("system", system_prompt.to_string())];
    for (sender_type, content) in rows {
        let role = if sender_type == "agent" {
            "assistant"
        } else {
            "user"
        };
        messages.push(ChatMessage::text(role, content.unwrap_or_default()));
    }
    Ok(messages)
}

async fn build_acp_prompt(
    pool: &SqlitePool,
    thread_id: &str,
    system_prompt: &str,
) -> anyhow::Result<String> {
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT sender_type, content FROM messages \
         WHERE thread_id = ? AND status = 'visible' ORDER BY seq ASC",
    )
    .bind(thread_id)
    .fetch_all(pool)
    .await?;

    let mut prompt = String::new();
    prompt.push_str(system_prompt);
    prompt.push_str("\n\nConversation:\n");
    for (sender_type, content) in rows {
        let role = if sender_type == "agent" {
            "assistant"
        } else {
            "user"
        };
        prompt.push_str(role);
        prompt.push_str(": ");
        prompt.push_str(&content.unwrap_or_default());
        prompt.push('\n');
    }
    Ok(prompt)
}

async fn build_resume_messages(
    pool: &SqlitePool,
    thread_id: &str,
    system_prompt: &str,
    interrupted_message_id: &str,
) -> anyhow::Result<Vec<ChatMessage>> {
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, sender_type, content FROM messages \
         WHERE thread_id = ? AND (status = 'visible' OR id = ?) \
         ORDER BY seq ASC",
    )
    .bind(thread_id)
    .bind(interrupted_message_id)
    .fetch_all(pool)
    .await?;

    let mut messages = vec![ChatMessage::text("system", system_prompt.to_string())];
    for (_id, sender_type, content) in rows {
        let role = if sender_type == "agent" {
            "assistant"
        } else {
            "user"
        };
        messages.push(ChatMessage::text(role, content.unwrap_or_default()));
    }
    messages.push(ChatMessage::text(
        "user",
        RESUME_CONTINUATION_PROMPT.to_string(),
    ));
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
