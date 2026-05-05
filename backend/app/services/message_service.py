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
from dataclasses import dataclass
from typing import Any
from uuid import UUID

from fastapi import Request
from langchain_core.messages import (
    AIMessage,
    BaseMessage,
    HumanMessage,
)
from langgraph.graph.state import CompiledStateGraph
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.agents import runtime
from app.agents.context import DEFAULT_RUNTIME_LIMITS, build_agent_system_message
from app.agents.router import resolve_all_mentions
from app.core.exceptions import ConflictError, NotFoundError
from app.db import SessionLocal
from app.llm.chat_model import resolve_chat_model
from app.models.agent import Agent
from app.models.group import Group
from app.models.group_agent import GroupAgent
from app.models.group_member import GroupMember
from app.models.message import Message
from app.models.thread import Thread
from app.models.user import User
from app.services import group_service, thread_service

logger = logging.getLogger(__name__)

CONTEXT_WINDOW = 20
SILENT_MARKER = "<SILENT>"
PSEUDO_TOOL_PLACEHOLDER = (
    "[Non-executed tool markup removed: this runtime did not execute a tool call.]"
)
WAITING_FOR_USER_WARNING = "Waiting for your input"

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
class MessageSendResult:
    user_message: Message
    agent_replies: list[Message]
    warnings: list[str]
    silent_turns: list[SilentAgentTurn]
    all_silent: bool
    waiting_for_user: bool = False


async def list_messages(
    db: AsyncSession, group_id: UUID, user: User, limit: int = 50
) -> list[Message]:
    await group_service.get_group(db, group_id, user)
    stmt = (
        select(Message)
        .where(
            Message.group_id == group_id,
            Message.status.in_(("visible", "interrupted")),
        )
        # Secondary sort by id is a tie-breaker for legacy rows that share
        # a created_at value (pre-0009 migration, all messages within one
        # request transaction collided on `now()`). New rows get distinct
        # timestamps via `clock_timestamp()`, but the tie-breaker keeps
        # ordering stable for old data and as defense-in-depth.
        .order_by(Message.created_at.asc(), Message.id.asc())
        .limit(limit)
    )
    return list(await db.scalars(stmt))


async def clear_group_history(db: AsyncSession, group_id: UUID, user: User) -> int:
    await group_service.assert_owner(db, group_id, user)
    running = await db.scalar(
        select(Thread).where(Thread.group_id == group_id, Thread.status == "running").limit(1)
    )
    if running is not None:
        raise ConflictError("cannot clear group history while a thread is running")

    messages = list(
        await db.scalars(
            select(Message).where(
                Message.group_id == group_id,
                Message.status.in_(("visible", "interrupted")),
            )
        )
    )
    interrupted_thread_ids = {
        m.thread_id
        for m in messages
        if m.thread_id is not None and m.status == "interrupted"
    }
    for message in messages:
        message.status = "cleared"
    if interrupted_thread_ids:
        paused_threads = list(
            await db.scalars(
                select(Thread).where(
                    Thread.id.in_(interrupted_thread_ids),
                    Thread.status == "paused",
                )
            )
        )
        for thread in paused_threads:
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
    thread_id: UUID,
    reply_to: UUID | None,
    status: str = "visible",
) -> Message:
    msg = Message(
        group_id=group_id,
        thread_id=thread_id,
        sender_type="agent",
        sender_id=agent.id,
        message_type="text",
        content=content,
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


async def _build_lc_input(
    db: AsyncSession,
    group: Group,
    group_agent: GroupAgent,
    agent: Agent,
    extra_user_text: str | None = None,
) -> list[BaseMessage]:
    """Build the LangChain message list for an agent invocation.

    - system = shared agent context (prompt, group, workspace, tools, skills).
    - history = last `CONTEXT_WINDOW` group messages (visible OR interrupted)
      in chronological order. All group members share the same history.
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
    system_message = await build_agent_system_message(
        db,
        agent,
        owner,
        group=group,
        group_agent=group_agent,
        runtime_limits={**DEFAULT_RUNTIME_LIMITS, "context_history_messages": CONTEXT_WINDOW},
    )

    sender_names = await _build_sender_names(db, group.id)
    my_id = str(agent.id)

    history_stmt = (
        select(Message)
        .where(
            Message.group_id == group.id,
            Message.status.in_(("visible", "interrupted")),
        )
        # `Message.id` is a tie-breaker for legacy rows that share a
        # `created_at` (see `list_messages` for the full rationale). We
        # sort DESC + LIMIT then `.reverse()`, so both keys must be DESC
        # here to keep the post-reverse ASC ordering self-consistent.
        .order_by(Message.created_at.desc(), Message.id.desc())
        .limit(CONTEXT_WINDOW)
    )
    history = list(await db.scalars(history_stmt))
    history.reverse()

    out: list[BaseMessage] = [system_message]
    for m in history:
        if m.content is None:
            continue
        sid = str(m.sender_id) if m.sender_id else None
        if m.sender_type == "agent" and sid == my_id:
            out.append(AIMessage(content=m.content))
        else:
            name = sender_names.get(sid or "", None) if sid else None
            prefix = f"[{name}]: " if name else ""
            out.append(HumanMessage(content=f"{prefix}{m.content}"))
    if extra_user_text:
        out.append(HumanMessage(content=extra_user_text))
    return out


def _is_silent_reply(group: Group, text: str) -> bool:
    return group.proactive_mode and text.strip() == SILENT_MARKER


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


def _agent_identity_payload(agent: Agent, group_agent: GroupAgent) -> dict[str, str]:
    return {
        "agent_id": str(agent.id),
        "display_name": group_agent.display_name or agent.name,
    }


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
        "reply_to_message_id": (
            str(m.reply_to_message_id) if m.reply_to_message_id else None
        ),
        "created_at": m.created_at.isoformat() if m.created_at else None,
    }


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
            all_silent=False,
        )

    graph = request.app.state.graph

    human_names = await _human_mention_names(db, group_id)
    sender_name = sender.name or ""
    agent_replies: list[Message] = []
    warnings: list[str] = []
    silent_turns: list[SilentAgentTurn] = []
    waiting_for_user = False
    proactive_reply_budget = len(resolved) * group.proactive_reply_multiplier
    visible_replies_used = 0
    spoke_previous_round = True
    round_idx = 0
    last_visible_agent_id: UUID | None = None
    while (not group.proactive_mode and round_idx < 1) or (
        group.proactive_mode
        and visible_replies_used < proactive_reply_budget
        and spoke_previous_round
    ):
        round_idx += 1
        spoke_this_round = False
        selected_participants = (
            resolved if round_idx == 1 else random.sample(resolved, k=len(resolved))
        )
        round_participants = _avoid_immediate_repeat_speaker(
            selected_participants,
            last_visible_agent_id if group.proactive_mode else None,
        )
        for group_agent, agent in round_participants:
            if group.proactive_mode and visible_replies_used >= proactive_reply_budget:
                break
            chat_thread = await thread_service.get_or_create_chat_thread(
                db, group_id, agent.id, sender.id
            )
            await thread_service.mark_running(db, chat_thread)
            try:
                input_messages = await _build_lc_input(db, group, group_agent, agent)
                chat_model = await resolve_chat_model(db, agent, streaming=False)
                response: AIMessage = await runtime.run(
                    graph=graph,
                    thread_id=str(chat_thread.id),
                    chat_model=chat_model,
                    input_messages=input_messages,
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
                agent_msg = await _persist_agent_message(
                    db, group_id, agent, visible_text, chat_thread.id, reply_to=user_msg.id
                )
                agent_replies.append(agent_msg)
                visible_replies_used += 1
                spoke_this_round = True
                last_visible_agent_id = agent.id
                waiting_for_user = _requests_human_input(
                    visible_text, human_names, sender_name
                )
                if waiting_for_user:
                    warnings.append(WAITING_FOR_USER_WARNING)
                await thread_service.mark_completed(db, chat_thread)
                if waiting_for_user:
                    break
            except Exception as exc:
                logger.exception("agent %s failed in group %s", agent.id, group_id)
                await thread_service.mark_failed(db, chat_thread)
                display = group_agent.display_name or agent.name
                warnings.append(f"agent '{display}' failed: {exc!s}")
        if waiting_for_user or not group.proactive_mode:
            break
        spoke_previous_round = spoke_this_round

    if group.proactive_mode and silent_turns and not agent_replies:
        warnings.append("No one replied")

    return MessageSendResult(
        user_message=user_msg,
        agent_replies=agent_replies,
        warnings=warnings,
        silent_turns=silent_turns,
        all_silent=group.proactive_mode and bool(silent_turns) and not agent_replies,
        waiting_for_user=waiting_for_user,
    )


async def _stream_one_agent(
    db: AsyncSession,
    graph: CompiledStateGraph[Any, Any, Any, Any],
    group: Group,
    group_agent: GroupAgent,
    agent: Agent,
    chat_thread: Thread,
    reply_to: UUID,
    human_names: set[str],
    sender_name: str,
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
        input_messages = await _build_lc_input(db, group, group_agent, agent)
        chat_model = await resolve_chat_model(db, agent, streaming=True)
        async for kind, payload in runtime.run_with_stream(
            graph=graph,
            thread_id=str(chat_thread.id),
            chat_model=chat_model,
            input_messages=input_messages,
        ):
            if kind == "token":
                chunks.append(payload)
                visible_so_far = _sanitize_streaming_visible_content("".join(chunks))
                if len(visible_so_far) <= emitted_visible_len:
                    continue
                delta = visible_so_far[emitted_visible_len:]
                emitted_visible_len = len(visible_so_far)
                yield {
                    "event": "token",
                    "data": json.dumps(
                        {"agent_id": agent_id_str, "delta": delta}
                    ),
                }
            elif kind == "done":
                final: AIMessage = payload
                text = (
                    final.content
                    if isinstance(final.content, str)
                    else "".join(chunks)
                )
                if _is_silent_reply(group, text):
                    yield {
                        "event": "agent_silent",
                        "data": json.dumps(_agent_identity_payload(agent, group_agent)),
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
                                }
                            ),
                        }
                    agent_msg = await _persist_agent_message(
                        db, group.id, agent, visible_text, chat_thread.id, reply_to=reply_to
                    )
                    yield {
                        "event": "agent_message",
                        "data": json.dumps(_serialize_msg(agent_msg)),
                    }
                    if _requests_human_input(visible_text, human_names, sender_name):
                        yield {
                            "event": "waiting_for_user",
                            "data": json.dumps({"message": WAITING_FOR_USER_WARNING}),
                        }
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
            await thread_service.mark_failed(db, chat_thread)
        raise


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
        yield {"event": "done", "data": ""}
        return

    graph = request.app.state.graph
    human_names = await _human_mention_names(db, group_id)
    sender_name = sender.name or ""
    emitted_agent_messages = 0
    proactive_reply_budget = len(resolved) * group.proactive_reply_multiplier
    spoke_previous_round = True
    round_idx = 0
    last_visible_agent_id: UUID | None = None
    waiting_for_user = False

    while (not group.proactive_mode and round_idx < 1) or (
        group.proactive_mode
        and emitted_agent_messages < proactive_reply_budget
        and spoke_previous_round
    ):
        round_idx += 1
        spoke_this_round = False
        selected_participants = (
            resolved if round_idx == 1 else random.sample(resolved, k=len(resolved))
        )
        round_participants = _avoid_immediate_repeat_speaker(
            selected_participants,
            last_visible_agent_id if group.proactive_mode else None,
        )
        for idx, (group_agent, agent) in enumerate(round_participants):
            if group.proactive_mode and emitted_agent_messages >= proactive_reply_budget:
                break
            display = group_agent.display_name or agent.name
            yield {
                "event": "agent_start",
                "data": json.dumps({
                    "agent_id": str(agent.id),
                    "display_name": display,
                    "index": idx,
                    "total": len(round_participants),
                    "round": round_idx,
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
                ):
                    if event["event"] == "agent_message":
                        emitted_agent_messages += 1
                        spoke_this_round = True
                        last_visible_agent_id = agent.id
                    elif event["event"] == "waiting_for_user":
                        waiting_for_user = True
                    yield event
                    if waiting_for_user:
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
                    }),
                }
            if waiting_for_user:
                break
        if waiting_for_user or not group.proactive_mode:
            break
        spoke_previous_round = spoke_this_round

    if group.proactive_mode and emitted_agent_messages == 0:
        yield {"event": "silence", "data": ""}
    yield {"event": "done", "data": ""}


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
        input_messages = await _build_lc_input(
            db, group, group_agent, agent, extra_user_text=RESUME_CONTINUATION_PROMPT
        )
        chat_model = await resolve_chat_model(db, agent, streaming=True)
        async for kind, payload in runtime.run_with_stream(
            graph=graph,
            thread_id=str(thread.id),
            chat_model=chat_model,
            input_messages=input_messages,
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
                addition = "".join(chunks)
                interrupted_msg.content = (interrupted_msg.content or "") + addition
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
