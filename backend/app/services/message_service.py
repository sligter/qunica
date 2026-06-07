"""Group message + multi-agent fan-out service.

Phase 1 Week 3-4 contract:
- User messages persist with `thread_id = NULL` (they live at the group root).
- Each @-mention resolves to an agent; the message triggers all matched
  agents sequentially in textual order, deduped by agent_id.
- Each agent reply persists with `thread_id` set to the per-(group, agent)
  `chat_thread`, lazily created on first reply.
- Each agent runs through `app.agents.runtime`, which wraps the LLM call in
  a single-node LangGraph and persists checkpoints via PostgresSaver.
- Per-invocation context for the LLM = system_prompt + group announcement +
  last 20 messages from the group (visible to all agents — this is how
  cross-agent collaboration works in V1).

Phase 1 Week 6 (interrupt + resume):
- `send_message_stream` catches `asyncio.CancelledError` mid-stream,
  persists whatever partial content was accumulated as
  `Message.status='interrupted'`, marks the chat_thread as `paused`, then
  re-raises so the underlying ASGI plumbing finalizes the connection.
  Subsequent agents in a fan-out are NOT run after an interrupt.
- `resume_thread_stream` picks up the most recent interrupted message for
  a paused thread, re-invokes the agent with a synthetic continuation
  prompt, and APPENDS new tokens to the existing message's content. On
  completion the thread + message return to `completed` / `visible`.
"""

import asyncio
import json
import logging
import random
import re
from collections.abc import AsyncIterator, Sequence
from contextlib import asynccontextmanager
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any
from uuid import UUID

from fastapi import Request
from langchain_core.messages import (
    AIMessage,
    BaseMessage,
    HumanMessage,
)
from sqlalchemy import select, tuple_, update
from sqlalchemy.ext.asyncio import AsyncSession

from app.agents import runtime
from app.agents.context import (
    DEFAULT_RUNTIME_LIMITS,
    AgentInvocationContext,
    build_agent_invocation_context,
)
from app.agents.context_budget import (
    ContextHistoryItem,
    build_budgeted_context,
    estimate_message_tokens,
    merge_rolling_summary,
    resolve_context_budget,
)
from app.agents.context_usage import (
    ContextUsage,
    extract_context_usage,
    fallback_context_usage,
)
from app.agents.router import resolve_all_mentions, resolve_explicit_mentions
from app.agents.runtime import RuntimeAgentHandoff, RuntimeToolEvent, RuntimeWaitForUser
from app.agents.workspace_tools import build_workspace_tools
from app.core.exceptions import AgentChatError, ConflictError, NotFoundError
from app.db import SessionLocal
from app.external_agents import (
    normalize_external_runtime,
    run_external_agent,
    run_external_agent_stream,
)
from app.llm.chat_model import resolve_chat_model
from app.models.agent import Agent
from app.models.group import Group
from app.models.group_agent import GroupAgent
from app.models.group_member import GroupMember
from app.models.message import Message
from app.models.thread import Thread
from app.models.user import User
from app.services import group_service, llm_provider_service, thread_service

logger = logging.getLogger(__name__)

CONTEXT_HISTORY_CANDIDATE_LIMIT = 200
SILENT_MARKER = "<SILENT>"
PSEUDO_TOOL_PLACEHOLDER = (
    "[Non-executed tool markup removed: this runtime did not execute a tool call.]"
)
WAITING_FOR_USER_WARNING = "Waiting for your input"
TOOL_CALL_STARTED_MESSAGE = "Tool call started"
TOOL_CALL_COMPLETED_MESSAGE = "Tool call completed"
MAX_AGENT_TOOL_DISPATCHES_PER_SEND = 8

_REASONING_BLOCK_RE = re.compile(r"<think\b[^>]*>.*?</think>", re.IGNORECASE | re.DOTALL)
_PSEUDO_TOOL_BLOCK_RE = re.compile(
    r"<(tool_call|tool_code)\b[^>]*>.*?</\1>", re.IGNORECASE | re.DOTALL
)

RESUME_CONTINUATION_PROMPT = (
    "Continue from your last reply. Pick up exactly where you left off; "
    "do not restart or repeat what you already said."
)


@dataclass
class SilentAgentTurn:
    agent_id: UUID
    display_name: str


@dataclass
class AgentToolDispatch:
    caller_agent: Agent
    helper_group_agent: GroupAgent
    helper_agent: Agent
    content: str


@dataclass
class BuiltAgentInvocation:
    input_messages: list[BaseMessage]
    context: AgentInvocationContext
    fallback_context_usage: ContextUsage
    context_message_id: UUID | None


@dataclass(frozen=True)
class ProviderContextConfig:
    model_name: str | None
    context_window_tokens: int | None
    output_reserve_ratio: float | None


@dataclass
class MessageSendResult:
    user_message: Message
    agent_replies: list[Message]
    warnings: list[str]
    silent_turns: list[SilentAgentTurn]
    dispatch_messages: list[Message]
    all_silent: bool
    waiting_for_user: bool = False


@asynccontextmanager
async def _db_lock_section(
    lock: asyncio.Lock | None,
) -> AsyncIterator[None]:
    if lock is None:
        yield
        return
    async with lock:
        yield


async def list_messages(
    db: AsyncSession,
    group_id: UUID,
    user: User,
    limit: int = 30,
    before: UUID | None = None,
) -> list[Message]:
    await group_service.get_group(db, group_id, user)
    limit = min(max(limit, 1), 100)
    visible_statuses = ("visible", "interrupted")
    before_message: Message | None = None
    if before is not None:
        before_message = await db.scalar(
            select(Message).where(
                Message.id == before,
                Message.group_id == group_id,
                Message.status.in_(visible_statuses),
            )
        )
        if before_message is None:
            raise NotFoundError("message not found")

    conditions = [
        Message.group_id == group_id,
        Message.status.in_(visible_statuses),
    ]
    if before_message is not None:
        conditions.append(
            tuple_(Message.created_at, Message.id) < (before_message.created_at, before_message.id)
        )

    stmt = (
        select(Message)
        .where(*conditions)
        .order_by(Message.created_at.desc(), Message.id.desc())
        .limit(limit)
    )
    messages = list(await db.scalars(stmt))
    return list(reversed(messages))


async def clear_group_history(db: AsyncSession, group_id: UUID, user: User) -> int:
    await group_service.assert_owner(db, group_id, user)
    group = await group_service.get_group(db, group_id, user)
    visible_statuses = ("visible", "interrupted")
    visible_message_thread_ids = (
        select(Message.thread_id)
        .where(
            Message.group_id == group_id,
            Message.thread_id.is_not(None),
            Message.status.in_(visible_statuses),
        )
        .distinct()
    )
    previously_cleared_thread_ids = (
        select(Message.thread_id)
        .where(
            Message.group_id == group_id,
            Message.thread_id.is_not(None),
            Message.status == "cleared",
        )
        .distinct()
    )
    running_visible_thread = await db.scalar(
        select(Thread)
        .where(
            Thread.group_id == group_id,
            Thread.status == "running",
            Thread.id.in_(visible_message_thread_ids),
            Thread.id.not_in(previously_cleared_thread_ids),
        )
        .limit(1)
    )
    if running_visible_thread is not None:
        raise ConflictError("cannot clear group history while a thread is running")

    messages = list(
        await db.scalars(
            select(Message).where(
                Message.group_id == group_id,
                Message.status.in_(visible_statuses),
            )
        )
    )
    thread_ids = {m.thread_id for m in messages if m.thread_id is not None}
    for message in messages:
        message.status = "cleared"
    group.context_summary = None
    group.context_summary_message_id = None
    group.context_summary_updated_at = None
    # Clearing the conversation resets each agent's context, so the last-known
    # usage baseline is no longer meaningful — drop it so the UI doesn't show
    # stale token counts for the fresh (empty) context.
    await db.execute(
        update(GroupAgent)
        .where(GroupAgent.group_id == group_id)
        .values(
            last_context_input_tokens=None,
            last_context_output_tokens=None,
            last_context_total_tokens=None,
            last_context_window_tokens=None,
            last_context_output_reserve_tokens=None,
            last_context_message_id=None,
            last_context_usage_source=None,
            last_context_updated_at=None,
        )
    )
    if thread_ids:
        threads_with_remaining_visible_messages = (
            select(Message.thread_id)
            .where(
                Message.group_id == group_id,
                Message.thread_id.in_(thread_ids),
                Message.status.in_(visible_statuses),
            )
            .distinct()
        )
        clearable_threads = list(
            await db.scalars(
                select(Thread).where(
                    Thread.id.in_(thread_ids),
                    Thread.status.in_(("running", "paused", "completed", "failed", "created")),
                    Thread.id.not_in(threads_with_remaining_visible_messages),
                )
            )
        )
        for thread in clearable_threads:
            thread.status = "cleared"
    await db.flush()
    return len(messages)


async def _persist_user_message(
    db: AsyncSession, group_id: UUID, sender: User, content: str
) -> Message:
    msg = Message(
        group_id=group_id,
        sender_type="user",
        sender_id=sender.id,
        message_type="text",
        content=content,
    )
    db.add(msg)
    await db.flush()
    await db.refresh(msg)
    return msg


async def _persist_agent_message(
    db: AsyncSession,
    group_id: UUID,
    agent: Agent,
    content: str,
    thread_id: UUID | None,
    reply_to: UUID | None,
    status: str = "visible",
    context_usage: ContextUsage | None = None,
) -> Message:
    content_json: dict[str, Any] | None = None
    if context_usage is not None:
        content_json = {"context_usage": context_usage.to_payload()}
    msg = Message(
        group_id=group_id,
        thread_id=thread_id,
        sender_type="agent",
        sender_id=agent.id,
        message_type="text",
        content=content,
        content_json=content_json,
        reply_to_message_id=reply_to,
        status=status,
    )
    db.add(msg)
    await db.flush()
    await db.refresh(msg)
    return msg


async def _build_sender_names(
    db: AsyncSession, group_id: UUID
) -> dict[str, str]:
    """Build a {str(sender_id): display_name} lookup for all group participants.

    Covers both agents (via GroupAgent.display_name / Agent.name) and
    human members (via User.name). Used to attribute shared chat history
    so each agent can tell who said what.
    """
    names: dict[str, str] = {}

    agent_rows = (
        await db.execute(
            select(GroupAgent, Agent)
            .join(Agent, Agent.id == GroupAgent.agent_id)
            .where(GroupAgent.group_id == group_id)
        )
    ).all()
    for ga, a in agent_rows:
        names[str(a.id)] = ga.display_name or a.name

    member_rows = (
        await db.execute(
            select(GroupMember, User)
            .join(User, User.id == GroupMember.user_id)
            .where(GroupMember.group_id == group_id)
        )
    ).all()
    for _gm, u in member_rows:
        names[str(u.id)] = u.name

    return names


async def _human_mention_names(db: AsyncSession, group_id: UUID) -> set[str]:
    rows = (
        await db.execute(
            select(GroupMember, User)
            .join(User, User.id == GroupMember.user_id)
            .where(GroupMember.group_id == group_id, GroupMember.status == "active")
        )
    ).all()
    names: set[str] = set()
    for _gm, user in rows:
        if user.name:
            names.add(user.name.casefold())
    return names


def _strip_incomplete_internal_markup(text: str) -> str:
    lowered = text.casefold()
    cut_positions = [
        pos
        for marker in ("<think", "<tool_call", "<tool_code")
        if (pos := lowered.rfind(marker)) != -1
        and lowered.find(">", pos) != -1
        and not re.search(rf"</{marker[1:]}>", lowered[pos:])
    ]
    if not cut_positions:
        return text
    return text[: min(cut_positions)]


def _sanitize_agent_visible_content(text: str) -> str:
    sanitized = _REASONING_BLOCK_RE.sub("", text)
    sanitized = _PSEUDO_TOOL_BLOCK_RE.sub(PSEUDO_TOOL_PLACEHOLDER, sanitized)
    sanitized = _strip_incomplete_internal_markup(sanitized)
    sanitized = re.sub(r"\n{3,}", "\n\n", sanitized).strip()
    return sanitized


def _sanitize_streaming_visible_content(text: str) -> str:
    sanitized = _REASONING_BLOCK_RE.sub("", text)
    sanitized = _PSEUDO_TOOL_BLOCK_RE.sub(PSEUDO_TOOL_PLACEHOLDER, sanitized)

    lowered = sanitized.casefold()
    cut_positions = [
        pos
        for marker in ("<think", "<tool_call", "<tool_code")
        if (pos := lowered.rfind(marker)) != -1
    ]
    if cut_positions:
        sanitized = sanitized[: min(cut_positions)]

    # Hold back a possible partial opening tag at the end of a token chunk so
    # streamed UI never flashes raw internal markup such as "<thi".
    for index in range(len(sanitized) - 1, -1, -1):
        if sanitized[index] == "<":
            fragment = sanitized[index:].casefold()
            if any(
                marker.startswith(fragment)
                for marker in ("<think", "<tool_call", "<tool_code")
            ):
                sanitized = sanitized[:index]
            break
    return sanitized


def _mention_matches_name(text: str, name: str) -> bool:
    if not name:
        return False
    pattern = re.compile(
        rf"@{re.escape(name)}(?=$|[\s,.;:!?，。！？、])",
        re.IGNORECASE,
    )
    return bool(pattern.search(text))


def _requests_human_input(text: str, human_names: set[str], sender_name: str) -> bool:
    visible = text.casefold()
    target_names = {name for name in human_names if name}
    if sender_name:
        target_names.add(sender_name.casefold())
    if any(_mention_matches_name(text, name) for name in human_names if name):
        return True
    input_verbs = ("provide", "upload", "paste", "send", "share", "attach")
    input_objects = ("content", "draft", "file", "input", "material", "details", "requirements")
    if any(name in visible for name in target_names):
        return any(verb in visible for verb in input_verbs) and any(
            obj in visible for obj in input_objects
        )
    return False


def _agent_model_override(agent: Agent) -> str | None:
    config = agent.llm_config
    if not isinstance(config, dict):
        return None
    model = config.get("model")
    if isinstance(model, str) and model.strip():
        return model.strip()
    return None


async def _resolve_provider_context_config(
    db: AsyncSession,
    agent: Agent,
) -> ProviderContextConfig:
    model_override = _agent_model_override(agent)
    if model_override is not None:
        return ProviderContextConfig(
            model_name=model_override,
            context_window_tokens=None,
            output_reserve_ratio=None,
        )
    if agent.llm_provider_id is None:
        return ProviderContextConfig(
            model_name=None,
            context_window_tokens=None,
            output_reserve_ratio=None,
        )
    provider = await llm_provider_service.get_for_use(db, agent.llm_provider_id)
    return ProviderContextConfig(
        model_name=provider.default_model,
        context_window_tokens=provider.context_window_tokens,
        output_reserve_ratio=provider.context_output_reserve_ratio,
    )


def _history_sender_label(message: Message, sender_names: dict[str, str]) -> str:
    sid = str(message.sender_id) if message.sender_id else None
    if sid:
        name = sender_names.get(sid)
        if name:
            return name
    if message.sender_type == "agent":
        return "Agent"
    if message.sender_type == "user":
        return "User"
    return "System"


def _context_history_item(
    message: Message,
    *,
    my_agent_id: str,
    sender_names: dict[str, str],
) -> ContextHistoryItem | None:
    if message.content is None:
        return None
    sid = str(message.sender_id) if message.sender_id else None
    label = _history_sender_label(message, sender_names)
    if message.sender_type == "agent" and sid == my_agent_id:
        lc_message: BaseMessage = AIMessage(content=message.content)
    else:
        prefix = f"[{label}]: " if label else ""
        lc_message = HumanMessage(content=f"{prefix}{message.content}")
    return ContextHistoryItem(
        message_id=message.id,
        created_at=message.created_at,
        sender_label=label,
        message=lc_message,
        raw_content=message.content,
    )


def _is_message_after_summary_boundary(item: ContextHistoryItem, boundary: Message) -> bool:
    if item.created_at > boundary.created_at:
        return True
    if item.created_at < boundary.created_at:
        return False
    return str(item.message_id) > str(boundary.id)


async def _dropped_items_after_summary_boundary(
    db: AsyncSession,
    group: Group,
    dropped_items: list[ContextHistoryItem],
) -> list[ContextHistoryItem]:
    if not dropped_items:
        return []
    if group.context_summary_message_id is None:
        return dropped_items
    boundary = await db.scalar(
        select(Message).where(
            Message.id == group.context_summary_message_id,
            Message.group_id == group.id,
        )
    )
    if boundary is None:
        return dropped_items
    return [
        item
        for item in dropped_items
        if _is_message_after_summary_boundary(item, boundary)
    ]


async def _update_group_context_summary(
    db: AsyncSession,
    group: Group,
    dropped_items: list[ContextHistoryItem],
) -> tuple[str | None, bool]:
    new_dropped_items = await _dropped_items_after_summary_boundary(db, group, dropped_items)
    if not new_dropped_items:
        return group.context_summary, False
    summary = merge_rolling_summary(group.context_summary, new_dropped_items)
    if summary is None:
        return group.context_summary, False
    group.context_summary = summary
    group.context_summary_message_id = new_dropped_items[-1].message_id
    group.context_summary_updated_at = datetime.now(UTC)
    await db.flush()
    return summary, True


async def _record_context_usage(
    db: AsyncSession,
    group_agent: GroupAgent,
    usage: ContextUsage,
    context_message_id: UUID | None,
) -> None:
    group_agent.last_context_input_tokens = usage.input_tokens
    group_agent.last_context_output_tokens = usage.output_tokens
    group_agent.last_context_total_tokens = usage.total_tokens
    group_agent.last_context_window_tokens = usage.context_window_tokens
    group_agent.last_context_output_reserve_tokens = usage.output_reserve_tokens
    group_agent.last_context_message_id = context_message_id
    group_agent.last_context_usage_source = usage.source
    group_agent.last_context_updated_at = datetime.now(UTC)
    await db.flush()


def _fallback_usage_from_previous(
    group_agent: GroupAgent,
    history_items: list[ContextHistoryItem],
    dropped_items: list[ContextHistoryItem],
    full_fallback_input_tokens: int,
) -> tuple[int, str]:
    if (
        group_agent.last_context_input_tokens is None
        or group_agent.last_context_message_id is None
        or group_agent.last_context_usage_source
        not in {"provider", "previous_provider_delta"}
    ):
        return full_fallback_input_tokens, "fallback_tokenizer"

    boundary_index = next(
        (
            index
            for index, item in enumerate(history_items)
            if item.message_id == group_agent.last_context_message_id
        ),
        None,
    )
    if boundary_index is None:
        return full_fallback_input_tokens, "fallback_tokenizer"

    dropped_ids = {item.message_id for item in dropped_items}
    delta_tokens = sum(
        estimate_message_tokens(item.message)
        for item in history_items[boundary_index + 1 :]
        if item.message_id not in dropped_ids
    )
    return (
        group_agent.last_context_input_tokens + delta_tokens,
        "previous_provider_delta",
    )


async def _build_invocation(
    db: AsyncSession,
    group: Group,
    group_agent: GroupAgent,
    agent: Agent,
    extra_user_text: str | None = None,
    required_message_id: UUID | None = None,
    history_statuses: tuple[str, ...] = ("visible", "interrupted"),
) -> BuiltAgentInvocation:
    """Build the LangChain message list for an agent invocation.

    - system = shared agent context (prompt, group, workspace, tools, skills).
    - history = recent group messages (visible OR interrupted) in chronological
      order, selected by token budget. All group members share the same history.
      - Current agent's own messages → AIMessage (so the LLM sees them as
        its own prior turns).
      - Other agents' / users' messages → HumanMessage with a `[Name]: `
        prefix so the agent can distinguish participants.
    - If `extra_user_text` is provided, it's appended as a HumanMessage at the
      end (used by the resume flow to inject a continuation cue without
      persisting it as a real message).
    """
    owner = await db.scalar(select(User).where(User.id == agent.owner_id))
    if owner is None:
        raise NotFoundError(f"user {agent.owner_id}")
    provider_context = await _resolve_provider_context_config(db, agent)
    context_budget = resolve_context_budget(
        agent,
        model_name=provider_context.model_name,
        provider_context_window_tokens=provider_context.context_window_tokens,
        provider_output_reserve_ratio=provider_context.output_reserve_ratio,
    )
    context = await build_agent_invocation_context(
        db,
        agent,
        owner,
        group=group,
        group_agent=group_agent,
        runtime_limits={
            **DEFAULT_RUNTIME_LIMITS,
            "context_history_messages": CONTEXT_HISTORY_CANDIDATE_LIMIT,
            "context_window_tokens": context_budget.context_window_tokens,
            "context_input_budget_tokens": context_budget.input_budget_tokens,
            "context_output_reserve_tokens": context_budget.output_reserve_tokens,
        },
    )
    system_message = context.to_system_message()

    sender_names = await _build_sender_names(db, group.id)
    my_id = str(agent.id)

    history_stmt = (
        select(Message)
        .where(
            Message.group_id == group.id,
            Message.status.in_(history_statuses),
        )
        # `Message.id` is a tie-breaker for legacy rows that share a
        # `created_at` (see `list_messages` for the full rationale). We
        # sort DESC + LIMIT then `.reverse()`, so both keys must be DESC
        # here to keep the post-reverse ASC ordering self-consistent.
        .order_by(Message.created_at.desc(), Message.id.desc())
        .limit(CONTEXT_HISTORY_CANDIDATE_LIMIT)
    )
    history = list(await db.scalars(history_stmt))
    history.reverse()

    history_items: list[ContextHistoryItem] = []
    for m in history:
        item = _context_history_item(
            m,
            my_agent_id=my_id,
            sender_names=sender_names,
        )
        if item is not None:
            history_items.append(item)

    budgeted = build_budgeted_context(
        system_message=system_message,
        history_items=history_items,
        rolling_summary=group.context_summary,
        context_budget=context_budget,
        required_message_id=required_message_id,
        extra_user_text=extra_user_text,
    )
    for _ in range(2):
        updated_summary, summary_changed = await _update_group_context_summary(
            db,
            group,
            budgeted.dropped_items,
        )
        if not summary_changed:
            break
        budgeted = build_budgeted_context(
            system_message=system_message,
            history_items=history_items,
            rolling_summary=updated_summary,
            context_budget=context_budget,
            required_message_id=required_message_id,
            extra_user_text=extra_user_text,
        )
    context_message_id = history_items[-1].message_id if history_items else required_message_id
    fallback_input_tokens, fallback_source = _fallback_usage_from_previous(
        group_agent,
        history_items,
        budgeted.dropped_items,
        budgeted.fallback_input_tokens,
    )
    return BuiltAgentInvocation(
        input_messages=budgeted.messages,
        context=context,
        fallback_context_usage=fallback_context_usage(
            input_tokens=fallback_input_tokens,
            context_budget=budgeted.context_budget,
            source=fallback_source,
        ),
        context_message_id=context_message_id,
    )


async def _build_lc_input(
    db: AsyncSession,
    group: Group,
    group_agent: GroupAgent,
    agent: Agent,
    extra_user_text: str | None = None,
) -> list[BaseMessage]:
    invocation = await _build_invocation(
        db, group, group_agent, agent, extra_user_text=extra_user_text
    )
    return invocation.input_messages


def _is_silent_reply(group: Group, text: str) -> bool:
    return group.proactive_mode and text.strip() == SILENT_MARKER


async def _resolve_bound_assistant(
    context: AgentInvocationContext,
    requested_agent_id: str,
) -> Agent:
    requested = requested_agent_id.strip()
    for assistant in context.assistant_agents:
        if requested in {str(assistant.id), assistant.name}:
            return assistant
    available = (
        ", ".join(f"{agent.name} ({agent.id})" for agent in context.assistant_agents)
        or "none"
    )
    raise AgentChatError(
        f"assistant agent {requested_agent_id!r} is not bound for this agent; "
        f"available: {available}"
    )


async def _resolve_group_assistant_member(
    db: AsyncSession,
    group: Group,
    caller_agent: Agent,
    caller_context: AgentInvocationContext,
    requested_agent_id: str,
) -> tuple[GroupAgent, Agent]:
    requested = requested_agent_id.strip()
    requested_folded = requested.casefold()
    bound_by_id = {assistant.id: assistant for assistant in caller_context.assistant_agents}
    if not bound_by_id:
        await _resolve_bound_assistant(caller_context, requested_agent_id)

    rows = (
        await db.execute(
            select(GroupAgent, Agent)
            .join(Agent, Agent.id == GroupAgent.agent_id)
            .where(
                GroupAgent.group_id == group.id,
                GroupAgent.agent_id.in_(list(bound_by_id)),
                GroupAgent.status == "active",
            )
        )
    ).all()
    for helper_group_agent, helper_agent in rows:
        candidate_names = {
            str(helper_agent.id),
            helper_agent.name,
            helper_group_agent.display_name or "",
        }
        if requested in candidate_names or requested_folded in {
            name.casefold() for name in candidate_names if name
        }:
            if helper_agent.id == caller_agent.id:
                raise AgentChatError("agent cannot delegate to itself")
            return helper_group_agent, helper_agent

    assistant = await _resolve_bound_assistant(caller_context, requested_agent_id)
    if assistant.id == caller_agent.id:
        raise AgentChatError("agent cannot delegate to itself")
    raise AgentChatError(
        f"assistant agent '{assistant.name}' must be added to this group before "
        "AgentAsTool can dispatch to it"
    )


def _build_agent_tool_dispatch_content(
    helper_group_agent: GroupAgent,
    helper_agent: Agent,
    caller_agent: Agent,
    task: str,
    instructions: str | None,
) -> str:
    stripped_task = task.strip()
    if not stripped_task:
        raise AgentChatError("agent-as-tool task must be non-empty")
    helper_display = helper_group_agent.display_name or helper_agent.name
    caller_display = caller_agent.name
    content = f"@{helper_display} {stripped_task}"
    if instructions and instructions.strip():
        content = (
            f"{content}\n\n"
            f"Instructions from @{caller_display}: {instructions.strip()}"
        )
    return content


def _agent_free_mention_dispatch_limit(group: Group) -> int:
    return max(0, group.agent_free_mention_max_dispatches)


async def _append_agent_reply_mentions(
    db: AsyncSession,
    group: Group,
    visible_text: str,
    *,
    current_agent_id: UUID,
    skip_agent_ids: set[UUID],
    next_participants: list[tuple[GroupAgent, Agent]],
    next_agent_ids: set[UUID],
    remaining_dispatches: int,
    budget_agent_ids: set[UUID] | None = None,
) -> int:
    if (
        not group.allow_agent_free_mention
        or remaining_dispatches <= 0
        or "@" not in visible_text
    ):
        return 0
    mentioned = await resolve_explicit_mentions(db, group, visible_text)
    added = 0
    for group_agent, agent in mentioned:
        if added >= remaining_dispatches:
            break
        if agent.id == current_agent_id:
            continue
        if agent.id in skip_agent_ids or agent.id in next_agent_ids:
            continue
        next_participants.append((group_agent, agent))
        next_agent_ids.add(agent.id)
        if budget_agent_ids is not None:
            budget_agent_ids.add(agent.id)
        added += 1
    return added


def _avoid_immediate_repeat_speaker(
    participants: Sequence[tuple[GroupAgent, Agent]],
    last_visible_agent_id: UUID | None,
) -> list[tuple[GroupAgent, Agent]]:
    ordered = list(participants)
    if last_visible_agent_id is None or len(ordered) < 2:
        return ordered
    first_group_agent, first_agent = ordered[0]
    if first_agent.id != last_visible_agent_id:
        return ordered
    for index, (_group_agent, agent) in enumerate(ordered[1:], start=1):
        if agent.id != last_visible_agent_id:
            return [
                *ordered[1 : index + 1],
                (first_group_agent, first_agent),
                *ordered[index + 1 :],
            ]
    return ordered


def _legacy_admin_agent_ids(group: Group) -> set[str]:
    return set(group.admin_agent_ids or [])


def _earliest_joined(
    participants: Sequence[tuple[GroupAgent, Agent]],
) -> tuple[GroupAgent, Agent] | None:
    if not participants:
        return None
    return min(participants, key=lambda item: (item[0].joined_at, item[0].id))


def _is_legacy_admin(group: Group, agent: Agent) -> bool:
    return str(agent.id) in _legacy_admin_agent_ids(group)


def _is_legacy_star_hub(group: Group, group_agent: GroupAgent, agent: Agent) -> bool:
    return group_agent.topology_role is None and _is_legacy_admin(group, agent)


def _is_hierarchical_leader(group: Group, group_agent: GroupAgent, agent: Agent) -> bool:
    return group_agent.topology_role == "leader" or (
        group_agent.topology_role is None and _is_legacy_admin(group, agent)
    )


def _order_star_participants(
    group: Group,
    participants: Sequence[tuple[GroupAgent, Agent]],
) -> list[tuple[GroupAgent, Agent]]:
    ordered = list(participants)
    hub = next((item for item in ordered if item[0].topology_role == "hub"), None)
    if hub is None:
        hub = next(
            (
                item
                for item in ordered
                if _is_legacy_star_hub(group, item[0], item[1])
            ),
            None,
        )
    if hub is None:
        hub = _earliest_joined(ordered)
    if hub is None:
        return ordered
    return [hub, *[item for item in ordered if item[1].id != hub[1].id]]


def _order_hierarchical_participants(
    group: Group,
    participants: Sequence[tuple[GroupAgent, Agent]],
) -> list[tuple[GroupAgent, Agent]]:
    ordered = list(participants)
    leaders = [
        item for item in ordered if _is_hierarchical_leader(group, item[0], item[1])
    ]
    leader_ids = {item[1].id for item in leaders}
    workers = [item for item in ordered if item[1].id not in leader_ids]
    return [*leaders, *workers]


def _order_ring_participants(
    participants: Sequence[tuple[GroupAgent, Agent]],
    last_visible_agent_id: UUID | None,
) -> list[tuple[GroupAgent, Agent]]:
    ordered = sorted(
        participants,
        key=lambda item: (
            item[0].speaking_order is None,
            item[0].speaking_order or 0,
            item[0].joined_at,
            item[0].id,
        ),
    )
    return _rotate_after_agent(ordered, last_visible_agent_id)


def _rotate_after_agent(
    participants: Sequence[tuple[GroupAgent, Agent]],
    last_visible_agent_id: UUID | None,
) -> list[tuple[GroupAgent, Agent]]:
    ordered = list(participants)
    if last_visible_agent_id is None or len(ordered) < 2:
        return ordered
    for index, (_group_agent, agent) in enumerate(ordered):
        if agent.id == last_visible_agent_id:
            start = (index + 1) % len(ordered)
            return [*ordered[start:], *ordered[:start]]
    return ordered


async def _latest_visible_agent_id(db: AsyncSession, group_id: UUID) -> UUID | None:
    latest = await db.scalar(
        select(Message)
        .where(
            Message.group_id == group_id,
            Message.sender_type == "agent",
            Message.sender_id.is_not(None),
            Message.status == "visible",
        )
        .order_by(Message.created_at.desc(), Message.id.desc())
        .limit(1)
    )
    return latest.sender_id if latest is not None else None


def _order_round_participants(
    group: Group,
    resolved: Sequence[tuple[GroupAgent, Agent]],
    *,
    round_idx: int,
    last_visible_agent_id: UUID | None,
) -> list[tuple[GroupAgent, Agent]]:
    mode = group.communication_mode or "mesh"
    if mode == "mesh":
        selected = (
            list(resolved)
            if round_idx == 1
            else random.sample(list(resolved), k=len(resolved))
        )
        return _avoid_immediate_repeat_speaker(
            selected,
            last_visible_agent_id if group.proactive_mode else None,
        )
    if mode == "star":
        return _order_star_participants(group, resolved)
    if mode == "hierarchical":
        return _order_hierarchical_participants(group, resolved)
    if mode == "ring":
        return _order_ring_participants(resolved, last_visible_agent_id)
    return list(resolved)


def _agent_identity_payload(
    agent: Agent,
    group_agent: GroupAgent,
    stream_id: UUID | None = None,
    round_idx: int | None = None,
) -> dict[str, Any]:
    payload: dict[str, Any] = {
        "agent_id": str(agent.id),
        "display_name": group_agent.display_name or agent.name,
    }
    if stream_id is not None:
        payload["stream_id"] = str(stream_id)
    if round_idx is not None:
        payload["round"] = round_idx
    return payload


def _is_waiting_for_user_response(response: AIMessage) -> bool:
    return bool(response.additional_kwargs.get("waiting_for_user"))


def _is_agent_handoff_response(response: AIMessage) -> bool:
    return bool(response.additional_kwargs.get("agent_handoff"))


def _waiting_message_from_response(response: AIMessage) -> str:
    message = response.additional_kwargs.get("waiting_message")
    return str(message) if message else WAITING_FOR_USER_WARNING


def _human_input_request_payload(message: str) -> dict[str, Any] | None:
    prefix = "Human input requested:"
    stripped = message.strip()
    if not stripped.casefold().startswith(prefix.casefold()):
        return None
    question = stripped[len(prefix) :].strip()
    if not question:
        return None
    return {"question": question, "required": True}


def _human_input_request_from_response(response: AIMessage) -> dict[str, Any] | None:
    value = response.additional_kwargs.get("human_input_request")
    if not isinstance(value, dict):
        return None
    question = value.get("question")
    if not isinstance(question, str) or not question.strip():
        return None
    payload: dict[str, Any] = {
        "question": question.strip(),
        "required": value.get("required") is not False,
    }
    choices = value.get("choices")
    if isinstance(choices, list):
        normalized_choices = [
            choice.strip()
            for choice in choices
            if isinstance(choice, str) and choice.strip()
        ][:8]
        if normalized_choices:
            payload["choices"] = normalized_choices
    return payload


def _tool_event_waits_for_user(tool_event: RuntimeToolEvent) -> bool:
    return tool_event.tool_name == "AskUser" and tool_event.status == "input_required"


def _serialize_tool_event(
    tool_event: RuntimeToolEvent,
    agent: Agent,
    group_agent: GroupAgent,
    stream_id: UUID | None = None,
    round_idx: int | None = None,
) -> dict[str, Any]:
    payload: dict[str, Any] = {
        "agent_id": str(agent.id),
        "display_name": group_agent.display_name or agent.name,
        "tool_call_id": tool_event.tool_call_id,
        "tool_name": tool_event.tool_name,
        "status": tool_event.status,
    }
    if tool_event.args_summary:
        payload["args_summary"] = tool_event.args_summary
    if tool_event.result_summary:
        payload["result_summary"] = tool_event.result_summary
    if tool_event.input_request is not None:
        input_request: dict[str, Any] = {
            "question": tool_event.input_request.question,
            "required": tool_event.input_request.required,
        }
        if tool_event.input_request.choices:
            input_request["choices"] = list(tool_event.input_request.choices)
        payload["input_request"] = input_request
    if stream_id is not None:
        payload["stream_id"] = str(stream_id)
    if round_idx is not None:
        payload["round"] = round_idx
    return payload


def _serialize_msg(m: Message) -> dict[str, Any]:
    return {
        "id": str(m.id),
        "group_id": str(m.group_id),
        "thread_id": str(m.thread_id) if m.thread_id else None,
        "sender_type": m.sender_type,
        "sender_id": str(m.sender_id) if m.sender_id else None,
        "message_type": m.message_type,
        "content": m.content,
        "status": m.status,
        "context_usage": m.context_usage,
        "reply_to_message_id": (
            str(m.reply_to_message_id) if m.reply_to_message_id else None
        ),
        "created_at": m.created_at.isoformat() if m.created_at else None,
    }


def _external_workspace_path(context: AgentInvocationContext) -> Path:
    workspace = context.workspace
    if workspace is None or workspace.backend_type != "local" or not workspace.local_path:
        raise AgentChatError("external CLI agents require a local workspace")
    return Path(workspace.local_path).resolve()


def _render_external_prompt(input_messages: list[BaseMessage]) -> str:
    rendered: list[str] = []
    for message in input_messages:
        role = "Message"
        if isinstance(message, AIMessage):
            role = "Assistant"
        elif isinstance(message, HumanMessage):
            role = "User"
        elif message.type == "system":
            role = "System"
        content = message.content
        if isinstance(content, str):
            text = content
        elif isinstance(content, list):
            text = "".join(
                item if isinstance(item, str) else str(item.get("text", ""))
                for item in content
                if isinstance(item, (str, dict))
            )
        else:
            text = str(content)
        rendered.append(f"{role}:\n{text}")
    return "\n\n".join(rendered)


async def _run_external_agent_once(
    db: AsyncSession,
    group: Group | None,
    agent: Agent,
    chat_thread: Thread | None,
    input_messages: list[BaseMessage],
    context: AgentInvocationContext,
) -> str:
    config = normalize_external_runtime(agent.external_runtime)
    return await run_external_agent(
        db,
        owner_id=agent.owner_id,
        group_id=group.id if group is not None else None,
        agent_id=agent.id,
        thread_id=chat_thread.id if chat_thread is not None else None,
        config=config,
        cwd=_external_workspace_path(context),
        prompt=_render_external_prompt(input_messages),
    )


async def send_message(
    db: AsyncSession,
    request: Request,
    group_id: UUID,
    sender: User,
    content: str,
) -> MessageSendResult:
    group = await group_service.get_group(db, group_id, sender)
    user_msg = await _persist_user_message(db, group_id, sender, content)

    resolved = await resolve_all_mentions(db, group, content)
    if not resolved:
        return MessageSendResult(
            user_message=user_msg,
            agent_replies=[],
            warnings=["no agent mentioned in this group"],
            silent_turns=[],
            dispatch_messages=[],
            all_silent=False,
        )

    graph = request.app.state.graph

    human_names = await _human_mention_names(db, group_id)
    sender_name = sender.name or ""
    agent_replies: list[Message] = []
    warnings: list[str] = []
    silent_turns: list[SilentAgentTurn] = []
    dispatch_messages: list[Message] = []
    pending_dispatches: list[AgentToolDispatch] = []
    dispatches_created = 0
    waiting_for_user = False
    handoff_dispatched = False
    budget_agent_ids = {agent.id for _group_agent, agent in resolved}
    visible_replies_used = 0
    mention_dispatches_used = 0
    mention_participants: list[tuple[GroupAgent, Agent]] = []
    spoke_previous_round = True
    round_idx = 0
    last_visible_agent_id = await _latest_visible_agent_id(db, group_id)
    while True:
        if mention_participants:
            round_idx += 1
            round_participants = mention_participants
            mention_participants = []
            is_mention_round = True
        else:
            proactive_reply_budget = len(budget_agent_ids) * group.proactive_reply_multiplier
            if not group.proactive_mode and round_idx >= 1:
                break
            if (
                group.proactive_mode
                and (visible_replies_used >= proactive_reply_budget or not spoke_previous_round)
            ):
                break
            round_idx += 1
            is_mention_round = False
            round_participants = _order_round_participants(
                group,
                resolved,
                round_idx=round_idx,
                last_visible_agent_id=last_visible_agent_id,
            )
        spoke_this_round = False
        next_mention_participants: list[tuple[GroupAgent, Agent]] = []
        next_mention_agent_ids: set[UUID] = set()
        for idx, (group_agent, agent) in enumerate(round_participants):
            if is_mention_round:
                if mention_dispatches_used >= _agent_free_mention_dispatch_limit(group):
                    break
                mention_dispatches_used += 1
            proactive_reply_budget = len(budget_agent_ids) * group.proactive_reply_multiplier
            if group.proactive_mode and visible_replies_used >= proactive_reply_budget:
                break
            remaining_round_agent_ids = {
                remaining_agent.id
                for _remaining_group_agent, remaining_agent in round_participants[idx + 1 :]
            }
            chat_thread = await thread_service.get_or_create_chat_thread(
                db, group_id, agent.id, sender.id
            )
            await thread_service.mark_running(db, chat_thread)
            try:
                invocation = await _build_invocation(
                    db,
                    group,
                    group_agent,
                    agent,
                    required_message_id=user_msg.id,
                )
                input_messages = invocation.input_messages
                context = invocation.context
                if agent.runtime_kind == "external_cli":
                    text = await _run_external_agent_once(
                        db,
                        group,
                        agent,
                        chat_thread,
                        input_messages,
                        context,
                    )
                    if _is_silent_reply(group, text):
                        silent_turns.append(
                            SilentAgentTurn(
                                agent_id=agent.id,
                                display_name=group_agent.display_name or agent.name,
                            )
                        )
                        await thread_service.mark_completed(db, chat_thread)
                        continue
                    visible_text = _sanitize_agent_visible_content(text)
                    if visible_text:
                        await _record_context_usage(
                            db,
                            group_agent,
                            invocation.fallback_context_usage,
                            invocation.context_message_id,
                        )
                        agent_msg = await _persist_agent_message(
                            db,
                            group_id,
                            agent,
                            visible_text,
                            chat_thread.id,
                            reply_to=user_msg.id,
                            context_usage=invocation.fallback_context_usage,
                        )
                        agent_replies.append(agent_msg)
                        visible_replies_used += 1
                        spoke_this_round = True
                        last_visible_agent_id = agent.id
                        await _append_agent_reply_mentions(
                            db,
                            group,
                            visible_text,
                            current_agent_id=agent.id,
                            skip_agent_ids={agent.id, *remaining_round_agent_ids},
                            next_participants=next_mention_participants,
                            next_agent_ids=next_mention_agent_ids,
                            remaining_dispatches=(
                                _agent_free_mention_dispatch_limit(group)
                                - mention_dispatches_used
                                - len(next_mention_participants)
                            ),
                            budget_agent_ids=budget_agent_ids if group.proactive_mode else None,
                        )
                    waiting_for_user = _requests_human_input(
                        visible_text, human_names, sender_name
                    )
                    if waiting_for_user:
                        warnings.append(WAITING_FOR_USER_WARNING)
                    await thread_service.mark_completed(db, chat_thread)
                    if waiting_for_user:
                        break
                    continue
                chat_model = await resolve_chat_model(db, agent, streaming=False)
                tool_requested_wait = False

                async def _record_tool_event(tool_event: RuntimeToolEvent) -> None:
                    nonlocal tool_requested_wait
                    if _tool_event_waits_for_user(tool_event):
                        tool_requested_wait = True

                current_agent = agent
                current_context = context

                async def _agent_tool_executor(
                    agent_id: str,
                    task: str,
                    instructions: str | None = None,
                    bound_agent: Agent = current_agent,
                    bound_context: AgentInvocationContext = current_context,
                ) -> str:
                    nonlocal dispatches_created
                    if dispatches_created >= MAX_AGENT_TOOL_DISPATCHES_PER_SEND:
                        raise AgentChatError("agent-as-tool dispatch limit reached for this send")
                    helper_group_agent, helper_agent = await _resolve_group_assistant_member(
                        db,
                        group,
                        bound_agent,
                        bound_context,
                        agent_id,
                    )
                    dispatch_content = _build_agent_tool_dispatch_content(
                        helper_group_agent,
                        helper_agent,
                        bound_agent,
                        task,
                        instructions,
                    )
                    pending_dispatches.append(
                        AgentToolDispatch(
                            caller_agent=bound_agent,
                            helper_group_agent=helper_group_agent,
                            helper_agent=helper_agent,
                            content=dispatch_content,
                        )
                    )
                    dispatches_created += 1
                    return json.dumps(
                        {
                            "tool": "AgentAsTool",
                            "status": "DISPATCHED",
                            "agent_id": str(helper_agent.id),
                            "display_name": helper_group_agent.display_name or helper_agent.name,
                            "dispatch": dispatch_content[:1000],
                            "message": (
                        "Visible group dispatch queued; the assistant will respond "
                        "through normal group routing."
                    ),
                        },
                        ensure_ascii=False,
                    )

                response: AIMessage = await runtime.run(
                    graph=graph,
                    thread_id=str(chat_thread.id),
                    chat_model=chat_model,
                    input_messages=input_messages,
                    workspace_tools=build_workspace_tools(
                        current_context,
                        agent_tool_executor=_agent_tool_executor,
                    ),
                    tool_event_callback=_record_tool_event,
                    agent_handoff_tool_names={"AgentAsTool"},
                )
                context_usage = extract_context_usage(
                    response,
                    fallback_usage=invocation.fallback_context_usage,
                )
                await _record_context_usage(
                    db,
                    group_agent,
                    context_usage,
                    invocation.context_message_id,
                )
                text = (
                    response.content
                    if isinstance(response.content, str)
                    else str(response.content)
                )
                if _is_silent_reply(group, text):
                    silent_turns.append(
                        SilentAgentTurn(
                            agent_id=agent.id,
                            display_name=group_agent.display_name or agent.name,
                        )
                    )
                    await thread_service.mark_completed(db, chat_thread)
                    continue
                visible_text = _sanitize_agent_visible_content(text)
                if _is_agent_handoff_response(response):
                    handoff_dispatched = True
                    await thread_service.mark_completed(db, chat_thread)
                    break
                if visible_text:
                    agent_msg = await _persist_agent_message(
                        db,
                        group_id,
                        agent,
                        visible_text,
                        chat_thread.id,
                        reply_to=user_msg.id,
                        context_usage=context_usage,
                    )
                    agent_replies.append(agent_msg)
                    visible_replies_used += 1
                    spoke_this_round = True
                    last_visible_agent_id = agent.id
                    await _append_agent_reply_mentions(
                        db,
                        group,
                        visible_text,
                        current_agent_id=agent.id,
                        skip_agent_ids={agent.id, *remaining_round_agent_ids},
                        next_participants=next_mention_participants,
                        next_agent_ids=next_mention_agent_ids,
                        remaining_dispatches=(
                            _agent_free_mention_dispatch_limit(group)
                            - mention_dispatches_used
                            - len(next_mention_participants)
                        ),
                        budget_agent_ids=budget_agent_ids if group.proactive_mode else None,
                    )
                waiting_for_user = (
                    tool_requested_wait
                    or _is_waiting_for_user_response(response)
                    or _requests_human_input(visible_text, human_names, sender_name)
                )
                if waiting_for_user:
                    warnings.append(_waiting_message_from_response(response))
                await thread_service.mark_completed(db, chat_thread)
                if waiting_for_user:
                    break
            except Exception as exc:
                logger.exception("agent %s failed in group %s", agent.id, group_id)
                await thread_service.mark_failed(db, chat_thread)
                display = group_agent.display_name or agent.name
                warnings.append(f"agent '{display}' failed: {exc!s}")
        if waiting_for_user or handoff_dispatched:
            break
        if next_mention_participants:
            mention_participants = next_mention_participants
            spoke_previous_round = spoke_this_round
            continue
        if not group.proactive_mode:
            break
        spoke_previous_round = spoke_this_round

    while pending_dispatches and not waiting_for_user:
        dispatch = pending_dispatches.pop(0)
        dispatch_msg = await _persist_agent_message(
            db,
            group_id,
            dispatch.caller_agent,
            dispatch.content,
            thread_id=None,
            reply_to=user_msg.id,
        )
        dispatch_messages.append(dispatch_msg)
        helper_thread = await thread_service.get_or_create_chat_thread(
            db, group_id, dispatch.helper_agent.id, sender.id
        )
        await thread_service.mark_running(db, helper_thread)
        try:
            invocation = await _build_invocation(
                db,
                group,
                dispatch.helper_group_agent,
                dispatch.helper_agent,
                required_message_id=dispatch_msg.id,
            )
            input_messages = invocation.input_messages
            context = invocation.context
            if dispatch.helper_agent.runtime_kind == "external_cli":
                text = await _run_external_agent_once(
                    db,
                    group,
                    dispatch.helper_agent,
                    helper_thread,
                    input_messages,
                    context,
                )
                if _is_silent_reply(group, text):
                    silent_turns.append(
                        SilentAgentTurn(
                            agent_id=dispatch.helper_agent.id,
                            display_name=(
                                dispatch.helper_group_agent.display_name
                                or dispatch.helper_agent.name
                            ),
                        )
                    )
                    await thread_service.mark_completed(db, helper_thread)
                    continue
                visible_text = _sanitize_agent_visible_content(text)
                if visible_text:
                    await _record_context_usage(
                        db,
                        dispatch.helper_group_agent,
                        invocation.fallback_context_usage,
                        invocation.context_message_id,
                    )
                    agent_msg = await _persist_agent_message(
                        db,
                        group_id,
                        dispatch.helper_agent,
                        visible_text,
                        helper_thread.id,
                        reply_to=dispatch_msg.id,
                        context_usage=invocation.fallback_context_usage,
                    )
                    agent_replies.append(agent_msg)
                waiting_for_user = _requests_human_input(
                    visible_text, human_names, sender_name
                )
                if waiting_for_user:
                    warnings.append(WAITING_FOR_USER_WARNING)
                await thread_service.mark_completed(db, helper_thread)
                continue
            chat_model = await resolve_chat_model(db, dispatch.helper_agent, streaming=False)
            tool_requested_wait = False

            async def _record_tool_event(tool_event: RuntimeToolEvent) -> None:
                nonlocal tool_requested_wait
                if _tool_event_waits_for_user(tool_event):
                    tool_requested_wait = True

            response = await runtime.run(
                graph=graph,
                thread_id=str(helper_thread.id),
                chat_model=chat_model,
                input_messages=input_messages,
                workspace_tools=build_workspace_tools(context),
                tool_event_callback=_record_tool_event,
            )
            context_usage = extract_context_usage(
                response,
                fallback_usage=invocation.fallback_context_usage,
            )
            await _record_context_usage(
                db,
                dispatch.helper_group_agent,
                context_usage,
                invocation.context_message_id,
            )
            text = response.content if isinstance(response.content, str) else str(response.content)
            if _is_silent_reply(group, text):
                silent_turns.append(
                    SilentAgentTurn(
                        agent_id=dispatch.helper_agent.id,
                        display_name=(
                            dispatch.helper_group_agent.display_name or dispatch.helper_agent.name
                        ),
                    )
                )
                await thread_service.mark_completed(db, helper_thread)
                continue
            visible_text = _sanitize_agent_visible_content(text)
            if visible_text:
                agent_msg = await _persist_agent_message(
                    db,
                    group_id,
                    dispatch.helper_agent,
                    visible_text,
                    helper_thread.id,
                    reply_to=dispatch_msg.id,
                    context_usage=context_usage,
                )
                agent_replies.append(agent_msg)
            waiting_for_user = (
                tool_requested_wait
                or _is_waiting_for_user_response(response)
                or _requests_human_input(visible_text, human_names, sender_name)
            )
            if waiting_for_user:
                warnings.append(_waiting_message_from_response(response))
            await thread_service.mark_completed(db, helper_thread)
        except Exception as exc:
            logger.exception(
                "agent %s failed in group %s dispatch", dispatch.helper_agent.id, group_id
            )
            await thread_service.mark_failed(db, helper_thread)
            display = dispatch.helper_group_agent.display_name or dispatch.helper_agent.name
            warnings.append(f"agent '{display}' failed: {exc!s}")

    if group.proactive_mode and silent_turns and not agent_replies:
        warnings.append("No one replied")

    return MessageSendResult(
        user_message=user_msg,
        agent_replies=agent_replies,
        warnings=warnings,
        silent_turns=silent_turns,
        dispatch_messages=dispatch_messages,
        all_silent=group.proactive_mode and bool(silent_turns) and not agent_replies,
        waiting_for_user=waiting_for_user,
    )


async def _stream_one_agent(
    db: AsyncSession,
    graph: Any,
    group: Group,
    group_agent: GroupAgent,
    agent: Agent,
    chat_thread: Thread,
    reply_to: UUID,
    human_names: set[str],
    sender_name: str,
    pending_dispatches: list[AgentToolDispatch] | None = None,
    dispatch_counter: list[int] | None = None,
    stream_id: UUID | None = None,
    round_idx: int | None = None,
    db_lock: asyncio.Lock | None = None,
) -> AsyncIterator[dict[str, str]]:
    """Stream one agent's reply, persisting on graceful done OR on cancel.

    Yields token + agent_message events. Caller wraps multiple invocations
    for fan-out.
    """
    agent_id_str = str(agent.id)
    chunks: list[str] = []
    emitted_visible_len = 0
    cancelled = False
    try:
        async with _db_lock_section(db_lock):
            invocation = await _build_invocation(
                db,
                group,
                group_agent,
                agent,
                required_message_id=reply_to,
            )
            input_messages = invocation.input_messages
            context = invocation.context
        # Emit the per-turn context usage as soon as the prompt is assembled, so
        # the avatar ring reflects THIS turn's input size in real time instead of
        # waiting for the final message (or showing a stale previous baseline).
        usage_payload: dict[str, Any] = {
            "agent_id": agent_id_str,
            "display_name": group_agent.display_name or agent.name,
            "context_usage": invocation.fallback_context_usage.to_payload(),
        }
        if stream_id is not None:
            usage_payload["stream_id"] = str(stream_id)
        if round_idx is not None:
            usage_payload["round"] = round_idx
        yield {"event": "context_usage", "data": json.dumps(usage_payload)}
        if agent.runtime_kind == "external_cli":
            config = normalize_external_runtime(agent.external_runtime)
            async with _db_lock_section(db_lock):
                external_events = run_external_agent_stream(
                    db,
                    owner_id=agent.owner_id,
                    group_id=group.id,
                    agent_id=agent.id,
                    thread_id=chat_thread.id,
                    config=config,
                    cwd=_external_workspace_path(context),
                    prompt=_render_external_prompt(input_messages),
                )
                async for event in external_events:
                    if event.kind == "run" and isinstance(event.data, dict):
                        payload = {
                            **event.data,
                            "agent_id": str(agent.id),
                            "display_name": group_agent.display_name or agent.name,
                        }
                        if stream_id is not None:
                            payload["stream_id"] = str(stream_id)
                        if round_idx is not None:
                            payload["round"] = round_idx
                        yield {"event": "external_agent_run", "data": json.dumps(payload)}
                        continue
                    if event.kind != "token" or not isinstance(event.data, str):
                        continue
                    chunks.append(event.data)
                    visible_so_far = _sanitize_streaming_visible_content("".join(chunks))
                    if len(visible_so_far) <= emitted_visible_len:
                        continue
                    delta = visible_so_far[emitted_visible_len:]
                    emitted_visible_len = len(visible_so_far)
                    yield {
                        "event": "token",
                        "data": json.dumps(
                            {"agent_id": agent_id_str, "delta": delta, "stream_id": str(stream_id)}
                            if stream_id is not None
                            else {"agent_id": agent_id_str, "delta": delta}
                        ),
                    }
            text = "".join(chunks)
            if _is_silent_reply(group, text):
                yield {
                    "event": "agent_silent",
                    "data": json.dumps(
                        _agent_identity_payload(agent, group_agent, stream_id, round_idx)
                    ),
                }
            else:
                visible_text = _sanitize_agent_visible_content(text)
                if len(visible_text) > emitted_visible_len:
                    yield {
                        "event": "token",
                        "data": json.dumps(
                            {
                                "agent_id": agent_id_str,
                                "delta": visible_text[emitted_visible_len:],
                                **({"stream_id": str(stream_id)} if stream_id is not None else {}),
                            }
                        ),
                    }
                if visible_text:
                    async with _db_lock_section(db_lock):
                        await _record_context_usage(
                            db,
                            group_agent,
                            invocation.fallback_context_usage,
                            invocation.context_message_id,
                        )
                        agent_msg = await _persist_agent_message(
                            db,
                            group.id,
                            agent,
                            visible_text,
                            chat_thread.id,
                            reply_to=reply_to,
                            context_usage=invocation.fallback_context_usage,
                        )
                    yield {
                        "event": "agent_message",
                        "data": json.dumps(_serialize_msg(agent_msg)),
                    }
                if _requests_human_input(visible_text, human_names, sender_name):
                    yield {
                        "event": "waiting_for_user",
                        "data": json.dumps(
                            {
                                "message": WAITING_FOR_USER_WARNING,
                                "agent_id": str(agent.id),
                                "display_name": group_agent.display_name or agent.name,
                                **({"stream_id": str(stream_id)} if stream_id is not None else {}),
                                **({"round": round_idx} if round_idx is not None else {}),
                            }
                        ),
                    }
            async with _db_lock_section(db_lock):
                await thread_service.mark_completed(db, chat_thread)
            return
        async with _db_lock_section(db_lock):
            chat_model = await resolve_chat_model(db, agent, streaming=True)
        async def _agent_tool_executor(
            agent_id: str, task: str, instructions: str | None = None
        ) -> str:
            if pending_dispatches is None or dispatch_counter is None:
                raise AgentChatError("group context is required for AgentAsTool dispatch")
            async with _db_lock_section(db_lock):
                if dispatch_counter[0] >= MAX_AGENT_TOOL_DISPATCHES_PER_SEND:
                    raise AgentChatError("agent-as-tool dispatch limit reached for this send")
                helper_group_agent, helper_agent = await _resolve_group_assistant_member(
                    db,
                    group,
                    agent,
                    context,
                    agent_id,
                )
                dispatch_content = _build_agent_tool_dispatch_content(
                    helper_group_agent,
                    helper_agent,
                    agent,
                    task,
                    instructions,
                )
                pending_dispatches.append(
                    AgentToolDispatch(
                        caller_agent=agent,
                        helper_group_agent=helper_group_agent,
                        helper_agent=helper_agent,
                        content=dispatch_content,
                    )
                )
                dispatch_counter[0] += 1
            return json.dumps(
                {
                    "tool": "AgentAsTool",
                    "status": "DISPATCHED",
                    "agent_id": str(helper_agent.id),
                    "display_name": helper_group_agent.display_name or helper_agent.name,
                    "dispatch": dispatch_content[:1000],
                    "message": (
                        "Visible group dispatch queued; the assistant will respond "
                        "through normal group routing."
                    ),
                },
                ensure_ascii=False,
            )

        async for kind, payload in runtime.run_with_stream(
            graph=graph,
            thread_id=str(chat_thread.id),
            chat_model=chat_model,
            input_messages=input_messages,
            workspace_tools=build_workspace_tools(
                context,
                agent_tool_executor=_agent_tool_executor,
            ),
            agent_handoff_tool_names={"AgentAsTool"},
        ):
            if kind == "tool_event" and isinstance(payload, RuntimeToolEvent):
                yield {
                    "event": (
                        "tool_call_start"
                        if payload.status == "started"
                        else "tool_call_result"
                    ),
                    "data": json.dumps(
                        _serialize_tool_event(payload, agent, group_agent, stream_id, round_idx)
                    ),
                }
            elif kind == "usage" and isinstance(payload, AIMessage):
                # One usage signal per model call in the tool loop — emit the
                # provider-reported context usage so the avatar ring climbs in
                # real time as the turn grows (persisted only on "done" below).
                step_usage = extract_context_usage(
                    payload,
                    fallback_usage=invocation.fallback_context_usage,
                )
                step_usage_payload: dict[str, Any] = {
                    "agent_id": agent_id_str,
                    "display_name": group_agent.display_name or agent.name,
                    "context_usage": step_usage.to_payload(),
                }
                if stream_id is not None:
                    step_usage_payload["stream_id"] = str(stream_id)
                if round_idx is not None:
                    step_usage_payload["round"] = round_idx
                yield {"event": "context_usage", "data": json.dumps(step_usage_payload)}
            elif kind == "reasoning":
                if isinstance(payload, str) and payload:
                    yield {
                        "event": "reasoning",
                        "data": json.dumps(
                            {
                                "agent_id": agent_id_str,
                                "delta": payload,
                                **({"stream_id": str(stream_id)} if stream_id is not None else {}),
                                **({"round": round_idx} if round_idx is not None else {}),
                            }
                        ),
                    }
            elif kind == "token":
                chunks.append(str(payload))
                visible_so_far = _sanitize_streaming_visible_content("".join(chunks))
                if len(visible_so_far) <= emitted_visible_len:
                    continue
                delta = visible_so_far[emitted_visible_len:]
                emitted_visible_len = len(visible_so_far)
                yield {
                    "event": "token",
                    "data": json.dumps(
                        {
                            "agent_id": agent_id_str,
                            "delta": delta,
                            **({"stream_id": str(stream_id)} if stream_id is not None else {}),
                        }
                    ),
                }
            elif kind == "agent_handoff" and isinstance(payload, RuntimeAgentHandoff):
                # The final response below terminates the caller turn. The
                # queued dispatch is drained by the group send loop as a separate
                # visible helper turn instead of feeding a tool result back to
                # the caller model.
                continue
            elif kind == "waiting_for_user" and isinstance(payload, RuntimeWaitForUser):
                # The final response is still persisted below; emit the public
                # waiting event after that message so frontend ordering remains
                # agent_message -> waiting_for_user -> done.
                continue
            elif kind == "done":
                final: AIMessage = payload
                context_usage = extract_context_usage(
                    final,
                    fallback_usage=invocation.fallback_context_usage,
                )
                async with _db_lock_section(db_lock):
                    await _record_context_usage(
                        db,
                        group_agent,
                        context_usage,
                        invocation.context_message_id,
                    )
                text = (
                    final.content
                    if isinstance(final.content, str)
                    else "".join(chunks)
                )
                if _is_silent_reply(group, text):
                    yield {
                        "event": "agent_silent",
                        "data": json.dumps(
                            _agent_identity_payload(agent, group_agent, stream_id, round_idx)
                        ),
                    }
                else:
                    visible_text = _sanitize_agent_visible_content(text)
                    if _is_agent_handoff_response(final):
                        if emitted_visible_len:
                            yield {
                                "event": "agent_silent",
                                "data": json.dumps(
                                    _agent_identity_payload(
                                        agent, group_agent, stream_id, round_idx
                                    )
                                ),
                            }
                        yield {
                            "event": "agent_handoff",
                            "data": json.dumps(
                                _agent_identity_payload(agent, group_agent, stream_id, round_idx)
                            ),
                        }
                        continue
                    if len(visible_text) > emitted_visible_len:
                        yield {
                            "event": "token",
                            "data": json.dumps(
                                {
                                    "agent_id": agent_id_str,
                                    "delta": visible_text[emitted_visible_len:],
                                    **(
                                        {"stream_id": str(stream_id)}
                                        if stream_id is not None
                                        else {}
                                    ),
                                }
                            ),
                        }
                    if visible_text:
                        async with _db_lock_section(db_lock):
                            agent_msg = await _persist_agent_message(
                                db,
                                group.id,
                                agent,
                                visible_text,
                                chat_thread.id,
                                reply_to=reply_to,
                                context_usage=context_usage,
                            )
                        yield {
                            "event": "agent_message",
                            "data": json.dumps(_serialize_msg(agent_msg)),
                        }
                    if _is_waiting_for_user_response(final) or _requests_human_input(
                        visible_text, human_names, sender_name
                    ):
                        waiting_message = _waiting_message_from_response(final)
                        input_request = _human_input_request_from_response(
                            final
                        ) or _human_input_request_payload(waiting_message)
                        yield {
                            "event": "waiting_for_user",
                            "data": json.dumps(
                                {
                                    "message": waiting_message,
                                    "agent_id": str(agent.id),
                                    "display_name": group_agent.display_name or agent.name,
                                    **(
                                        {"input_request": input_request}
                                        if input_request is not None
                                        else {}
                                    ),
                                    **(
                                        {"stream_id": str(stream_id)}
                                        if stream_id is not None
                                        else {}
                                    ),
                                    **({"round": round_idx} if round_idx is not None else {}),
                                }
                            ),
                        }
        async with _db_lock_section(db_lock):
            await thread_service.mark_completed(db, chat_thread)
    except asyncio.CancelledError:
        cancelled = True
        partial = _sanitize_agent_visible_content("".join(chunks))
        # The original `db` session is bound to the request whose task is
        # being torn down — committing on it is unreliable (asyncpg
        # connection state is in flux during cancellation, and `get_db`'s
        # `except Exception` doesn't cover BaseException so the implicit
        # close rolls back). Open a fresh session, do the persist there,
        # and commit it. Shielded so the cancellation can't tear *that*
        # task down too.
        async def _persist_on_cancel() -> None:
            async with SessionLocal() as fresh:
                if partial:
                    msg = Message(
                        group_id=group.id,
                        thread_id=chat_thread.id,
                        sender_type="agent",
                        sender_id=agent.id,
                        message_type="text",
                        content=partial,
                        reply_to_message_id=reply_to,
                        status="interrupted",
                    )
                    fresh.add(msg)
                t = await fresh.scalar(
                    select(Thread).where(Thread.id == chat_thread.id)
                )
                if t is not None:
                    t.status = "paused"
                await fresh.commit()

        await asyncio.shield(_persist_on_cancel())
        raise
    except Exception:
        if not cancelled:
            async with _db_lock_section(db_lock):
                await thread_service.mark_failed(db, chat_thread)
        raise


def _can_parallel_stream_round(
    group: Group,
    participants: Sequence[tuple[GroupAgent, Agent]],
) -> bool:
    _ = (group, participants)
    # Group chat context is a shared transcript: later agents must be able to
    # see earlier visible replies from the same user send. Parallel streaming
    # hides sibling replies because each worker builds its prompt from history
    # before those replies are persisted.
    return False


async def _stream_agent_round_parallel(
    db: AsyncSession,
    graph: Any,
    group: Group,
    group_id: UUID,
    sender: User,
    user_msg: Message,
    participants: Sequence[tuple[GroupAgent, Agent]],
    human_names: set[str],
    sender_name: str,
    pending_dispatches: list[AgentToolDispatch],
    dispatch_counter: list[int],
    round_idx: int,
) -> AsyncIterator[dict[str, str]]:
    db_lock = asyncio.Lock()
    queue: asyncio.Queue[dict[str, str] | None] = asyncio.Queue()
    stream_id = user_msg.id
    runs: list[tuple[int, GroupAgent, Agent, Thread]] = []

    for idx, (group_agent, agent) in enumerate(participants):
        async with _db_lock_section(db_lock):
            chat_thread = await thread_service.get_or_create_chat_thread(
                db, group_id, agent.id, sender.id
            )
        runs.append((idx, group_agent, agent, chat_thread))
        display = group_agent.display_name or agent.name
        yield {
            "event": "agent_start",
            "data": json.dumps(
                {
                    "agent_id": str(agent.id),
                    "display_name": display,
                    "index": idx,
                    "total": len(participants),
                    "round": round_idx,
                    "stream_id": str(stream_id),
                }
            ),
        }

    async def _worker(
        idx: int,
        group_agent: GroupAgent,
        agent: Agent,
        chat_thread: Thread,
    ) -> None:
        display = group_agent.display_name or agent.name
        try:
            async with _db_lock_section(db_lock):
                await thread_service.mark_running(db, chat_thread)
            async for event in _stream_one_agent(
                db,
                graph,
                group,
                group_agent,
                agent,
                chat_thread,
                reply_to=user_msg.id,
                human_names=human_names,
                sender_name=sender_name,
                pending_dispatches=pending_dispatches,
                dispatch_counter=dispatch_counter,
                stream_id=stream_id,
                round_idx=round_idx,
                db_lock=db_lock,
            ):
                await queue.put(event)
        except asyncio.CancelledError:
            raise
        except Exception as exc:
            logger.exception("agent %s failed in group %s parallel stream", agent.id, group_id)
            await queue.put(
                {
                    "event": "agent_error",
                    "data": json.dumps(
                        {
                            "agent_id": str(agent.id),
                            "display_name": display,
                            "error": str(exc),
                            "stream_id": str(stream_id),
                            "round": round_idx,
                        }
                    ),
                }
            )
        finally:
            await queue.put(None)

    tasks = [
        asyncio.create_task(_worker(idx, group_agent, agent, chat_thread))
        for idx, group_agent, agent, chat_thread in runs
    ]
    remaining = len(tasks)
    try:
        while remaining:
            item = await queue.get()
            if item is None:
                remaining -= 1
                continue
            yield item
    finally:
        pending = [task for task in tasks if not task.done()]
        for task in pending:
            task.cancel()
        if pending:
            await asyncio.gather(*pending, return_exceptions=True)


async def send_message_stream(
    db: AsyncSession,
    request: Request,
    group_id: UUID,
    sender: User,
    content: str,
) -> AsyncIterator[dict[str, str]]:
    """Yield SSE events: user_message → (agent_start + token×N + agent_message) per agent → done.

    Token events carry JSON `{"agent_id": ..., "delta": ...}` so the client
    knows which agent in the fan-out is currently speaking.

    An `agent_start` event is emitted before each agent begins streaming,
    carrying `{"agent_id": ..., "display_name": ...}` so the frontend can
    show a typing indicator.

    On client disconnect mid-stream: the currently streaming agent's partial
    reply is persisted with `status='interrupted'`, the chat_thread is
    marked `paused`, and the rest of the fan-out (if any) is skipped.

    Individual agent errors are caught and emitted as `agent_error` events;
    the fan-out continues with the next agent.
    """
    group = await group_service.get_group(db, group_id, sender)
    user_msg = await _persist_user_message(db, group_id, sender, content)
    yield {"event": "user_message", "data": json.dumps(_serialize_msg(user_msg))}

    resolved = await resolve_all_mentions(db, group, content)
    if not resolved:
        yield {"event": "warning", "data": "no agent matched in this group"}
        yield {"event": "done", "data": json.dumps({"stream_id": str(user_msg.id)})}
        return

    graph = request.app.state.graph
    human_names = await _human_mention_names(db, group_id)
    sender_name = sender.name or ""
    emitted_agent_messages = 0
    budget_agent_ids = {agent.id for _group_agent, agent in resolved}
    mention_dispatches_used = 0
    mention_participants: list[tuple[GroupAgent, Agent]] = []
    spoke_previous_round = True
    round_idx = 0
    last_visible_agent_id = await _latest_visible_agent_id(db, group_id)
    waiting_for_user = False
    handoff_dispatched = False
    pending_dispatches: list[AgentToolDispatch] = []
    dispatch_counter = [0]

    while True:
        if mention_participants:
            round_idx += 1
            round_participants = mention_participants
            mention_participants = []
            is_mention_round = True
        else:
            proactive_reply_budget = len(budget_agent_ids) * group.proactive_reply_multiplier
            if not group.proactive_mode and round_idx >= 1:
                break
            if (
                group.proactive_mode
                and (emitted_agent_messages >= proactive_reply_budget or not spoke_previous_round)
            ):
                break
            round_idx += 1
            is_mention_round = False
            round_participants = _order_round_participants(
                group,
                resolved,
                round_idx=round_idx,
                last_visible_agent_id=last_visible_agent_id,
            )
        spoke_this_round = False
        next_mention_participants: list[tuple[GroupAgent, Agent]] = []
        next_mention_agent_ids: set[UUID] = set()
        if _can_parallel_stream_round(group, round_participants):
            async for event in _stream_agent_round_parallel(
                db,
                graph,
                group,
                group_id,
                sender,
                user_msg,
                round_participants,
                human_names,
                sender_name,
                pending_dispatches,
                dispatch_counter,
                round_idx,
            ):
                if event["event"] == "agent_message":
                    emitted_agent_messages += 1
                    spoke_this_round = True
                    payload = json.loads(event["data"])
                    sender_id = payload.get("sender_id")
                    if isinstance(sender_id, str):
                        last_visible_agent_id = UUID(sender_id)
                elif event["event"] == "agent_handoff":
                    handoff_dispatched = True
                    continue
                elif event["event"] == "waiting_for_user":
                    waiting_for_user = True
                yield event
            if waiting_for_user or handoff_dispatched:
                break
            if not group.proactive_mode:
                break
            spoke_previous_round = spoke_this_round
            continue
        for idx, (group_agent, agent) in enumerate(round_participants):
            if is_mention_round:
                if mention_dispatches_used >= _agent_free_mention_dispatch_limit(group):
                    break
                mention_dispatches_used += 1
            proactive_reply_budget = len(budget_agent_ids) * group.proactive_reply_multiplier
            if group.proactive_mode and emitted_agent_messages >= proactive_reply_budget:
                break
            remaining_round_agent_ids = {
                remaining_agent.id
                for _remaining_group_agent, remaining_agent in round_participants[idx + 1 :]
            }
            display = group_agent.display_name or agent.name
            yield {
                "event": "agent_start",
                "data": json.dumps({
                    "agent_id": str(agent.id),
                    "display_name": display,
                    "index": idx,
                    "total": len(round_participants),
                    "round": round_idx,
                    "stream_id": str(user_msg.id),
                }),
            }
            chat_thread = await thread_service.get_or_create_chat_thread(
                db, group_id, agent.id, sender.id
            )
            await thread_service.mark_running(db, chat_thread)
            try:
                async for event in _stream_one_agent(
                    db,
                    graph,
                    group,
                    group_agent,
                    agent,
                    chat_thread,
                    reply_to=user_msg.id,
                    human_names=human_names,
                    sender_name=sender_name,
                    pending_dispatches=pending_dispatches,
                    dispatch_counter=dispatch_counter,
                    stream_id=user_msg.id,
                    round_idx=round_idx,
                ):
                    if event["event"] == "agent_message":
                        emitted_agent_messages += 1
                        spoke_this_round = True
                        last_visible_agent_id = agent.id
                        payload = json.loads(event["data"])
                        content_value = payload.get("content")
                        if isinstance(content_value, str):
                            await _append_agent_reply_mentions(
                                db,
                                group,
                                content_value,
                                current_agent_id=agent.id,
                                skip_agent_ids={agent.id, *remaining_round_agent_ids},
                                next_participants=next_mention_participants,
                                next_agent_ids=next_mention_agent_ids,
                                remaining_dispatches=(
                                    _agent_free_mention_dispatch_limit(group)
                                    - mention_dispatches_used
                                    - len(next_mention_participants)
                                ),
                                budget_agent_ids=(
                                    budget_agent_ids if group.proactive_mode else None
                                ),
                            )
                    elif event["event"] == "agent_handoff":
                        handoff_dispatched = True
                        continue
                    elif event["event"] == "waiting_for_user":
                        waiting_for_user = True
                    yield event
                    if waiting_for_user or handoff_dispatched:
                        break
            except asyncio.CancelledError:
                raise
            except Exception as exc:
                logger.exception("agent %s failed in group %s stream", agent.id, group_id)
                yield {
                    "event": "agent_error",
                    "data": json.dumps({
                        "agent_id": str(agent.id),
                        "display_name": display,
                        "error": str(exc),
                        "stream_id": str(user_msg.id),
                        "round": round_idx,
                    }),
                }
            if waiting_for_user or handoff_dispatched:
                break
        if waiting_for_user or handoff_dispatched:
            break
        if next_mention_participants:
            mention_participants = next_mention_participants
            spoke_previous_round = spoke_this_round
            continue
        if not group.proactive_mode:
            break
        spoke_previous_round = spoke_this_round

    while pending_dispatches and not waiting_for_user:
        dispatch = pending_dispatches.pop(0)
        dispatch_msg = await _persist_agent_message(
            db,
            group_id,
            dispatch.caller_agent,
            dispatch.content,
            thread_id=None,
            reply_to=user_msg.id,
        )
        yield {"event": "agent_message", "data": json.dumps(_serialize_msg(dispatch_msg))}
        display = dispatch.helper_group_agent.display_name or dispatch.helper_agent.name
        yield {
            "event": "agent_start",
            "data": json.dumps({
                "agent_id": str(dispatch.helper_agent.id),
                "display_name": display,
                "index": 0,
                "total": 1,
                "round": round_idx + 1,
                "stream_id": str(dispatch_msg.id),
            }),
        }
        helper_thread = await thread_service.get_or_create_chat_thread(
            db, group_id, dispatch.helper_agent.id, sender.id
        )
        await thread_service.mark_running(db, helper_thread)
        try:
            async for event in _stream_one_agent(
                db,
                graph,
                group,
                dispatch.helper_group_agent,
                dispatch.helper_agent,
                helper_thread,
                reply_to=dispatch_msg.id,
                human_names=human_names,
                sender_name=sender_name,
                pending_dispatches=None,
                dispatch_counter=None,
                stream_id=dispatch_msg.id,
                round_idx=round_idx + 1,
            ):
                if event["event"] == "agent_message":
                    emitted_agent_messages += 1
                elif event["event"] == "waiting_for_user":
                    waiting_for_user = True
                yield event
                if waiting_for_user:
                    break
        except asyncio.CancelledError:
            raise
        except Exception as exc:
            logger.exception(
                "agent %s failed in group %s dispatch stream",
                dispatch.helper_agent.id,
                group_id,
            )
            yield {
                "event": "agent_error",
                "data": json.dumps({
                    "agent_id": str(dispatch.helper_agent.id),
                    "display_name": display,
                    "error": str(exc),
                    "stream_id": str(dispatch_msg.id),
                    "round": round_idx + 1,
                }),
            }

    if group.proactive_mode and emitted_agent_messages == 0:
        yield {"event": "silence", "data": json.dumps({"stream_id": str(user_msg.id)})}
    yield {"event": "done", "data": json.dumps({"stream_id": str(user_msg.id)})}


async def _latest_interrupted_message(
    db: AsyncSession, thread_id: UUID
) -> Message | None:
    stmt = (
        select(Message)
        .where(Message.thread_id == thread_id, Message.status == "interrupted")
        .order_by(Message.created_at.desc())
        .limit(1)
    )
    result = await db.scalar(stmt)
    return result if isinstance(result, Message) else None


async def resume_thread_stream(
    db: AsyncSession,
    request: Request,
    thread: Thread,
    group_agent: GroupAgent,
    agent: Agent,
    group: Group,
    interrupted_msg: Message,
) -> AsyncIterator[dict[str, str]]:
    """Resume a paused thread by re-invoking the agent.

    Caller MUST pre-validate (thread.status == 'paused', user is a member,
    interrupted_msg exists). The endpoint does this so HTTP errors can
    surface correctly before the SSE response begins streaming.

    The agent's last interrupted message is the target: any new tokens are
    APPENDED to that message's content and the message is flipped back to
    `visible` once streaming completes successfully. The thread cycles
    `paused` → `running` → `completed` (or stays `paused` if cancelled
    again).
    """
    await thread_service.mark_running(db, thread)
    graph = request.app.state.graph

    chunks: list[str] = []
    agent_id_str = str(agent.id)
    try:
        invocation = await _build_invocation(
            db,
            group,
            group_agent,
            agent,
            extra_user_text=RESUME_CONTINUATION_PROMPT,
            required_message_id=interrupted_msg.id,
        )
        input_messages = invocation.input_messages
        context = invocation.context
        chat_model = await resolve_chat_model(db, agent, streaming=True)
        async for kind, payload in runtime.run_with_stream(
            graph=graph,
            thread_id=str(thread.id),
            chat_model=chat_model,
            input_messages=input_messages,
            workspace_tools=build_workspace_tools(context),
        ):
            if kind == "token":
                chunks.append(payload)
                yield {
                    "event": "token",
                    "data": json.dumps(
                        {"agent_id": agent_id_str, "delta": payload}
                    ),
                }
            elif kind == "done":
                final: AIMessage = payload
                context_usage = extract_context_usage(
                    final,
                    fallback_usage=invocation.fallback_context_usage,
                )
                await _record_context_usage(
                    db,
                    group_agent,
                    context_usage,
                    invocation.context_message_id,
                )
                addition = "".join(chunks)
                interrupted_msg.content = (interrupted_msg.content or "") + addition
                interrupted_msg.content_json = {
                    **(interrupted_msg.content_json or {}),
                    "context_usage": context_usage.to_payload(),
                }
                interrupted_msg.status = "visible"
                await db.flush()
                await db.refresh(interrupted_msg)
                yield {
                    "event": "agent_message",
                    "data": json.dumps(_serialize_msg(interrupted_msg)),
                }
        await thread_service.mark_completed(db, thread)
    except asyncio.CancelledError:
        addition = "".join(chunks)
        target_id = interrupted_msg.id
        thread_id_local = thread.id

        async def _persist_resume_cancel() -> None:
            async with SessionLocal() as fresh:
                if addition:
                    msg = await fresh.scalar(
                        select(Message).where(Message.id == target_id)
                    )
                    if msg is not None:
                        msg.content = (msg.content or "") + addition
                t = await fresh.scalar(
                    select(Thread).where(Thread.id == thread_id_local)
                )
                if t is not None:
                    t.status = "paused"
                await fresh.commit()

        await asyncio.shield(_persist_resume_cancel())
        raise
    except Exception:
        await thread_service.mark_failed(db, thread)
        raise

    yield {"event": "done", "data": ""}


async def resolve_resume_target(
    db: AsyncSession, thread_id: UUID, user: User
) -> tuple[Thread, GroupAgent, Agent, Group, Message]:
    """Pre-flight validation for the resume endpoint.

    Raises NotFoundError / ConflictError / PermissionDeniedError so that
    FastAPI exception handlers can return proper HTTP status codes BEFORE
    the SSE response starts streaming.
    """
    thread = await thread_service.get_thread(db, thread_id)
    if thread.status != "paused":
        raise ConflictError(f"thread {thread_id} is not paused")
    if thread.agent_id is None:
        raise ConflictError(f"thread {thread_id} has no agent")
    group = await group_service.get_group(db, thread.group_id, user)
    row = await db.execute(
        select(GroupAgent, Agent)
        .join(Agent, Agent.id == GroupAgent.agent_id)
        .where(
            GroupAgent.group_id == group.id,
            GroupAgent.agent_id == thread.agent_id,
            GroupAgent.status == "active",
        )
    )
    result = row.one_or_none()
    if result is None:
        raise NotFoundError(f"agent {thread.agent_id}")
    group_agent, agent = result
    interrupted_msg = await _latest_interrupted_message(db, thread.id)
    if interrupted_msg is None:
        raise ConflictError(
            f"thread {thread_id} has no interrupted message to resume"
        )
    return thread, group_agent, agent, group, interrupted_msg
