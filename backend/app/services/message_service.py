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
from collections.abc import AsyncIterator
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
from app.models.message import Message
from app.models.thread import Thread
from app.models.user import User
from app.services import group_service, thread_service

CONTEXT_WINDOW = 20

RESUME_CONTINUATION_PROMPT = (
    "Continue from your last reply. Pick up exactly where you left off; "
    "do not restart or repeat what you already said."
)


@dataclass
class MessageSendResult:
    user_message: Message
    agent_replies: list[Message]
    warnings: list[str]


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
        .order_by(Message.created_at.asc())
        .limit(limit)
    )
    return list(await db.scalars(stmt))


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


async def _build_lc_input(
    db: AsyncSession,
    group: Group,
    agent: Agent,
    extra_user_text: str | None = None,
) -> list[BaseMessage]:
    """Build the LangChain message list for an agent invocation.

    - system = shared agent context (prompt, group, workspace, tools, skills).
    - history = last `CONTEXT_WINDOW` group messages (visible OR interrupted)
      in chronological order, mapped to AIMessage / HumanMessage by sender_type.
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
        runtime_limits={**DEFAULT_RUNTIME_LIMITS, "context_history_messages": CONTEXT_WINDOW},
    )

    history_stmt = (
        select(Message)
        .where(
            Message.group_id == group.id,
            Message.status.in_(("visible", "interrupted")),
        )
        .order_by(Message.created_at.desc())
        .limit(CONTEXT_WINDOW)
    )
    history = list(await db.scalars(history_stmt))
    history.reverse()

    out: list[BaseMessage] = [system_message]
    for m in history:
        if m.content is None:
            continue
        if m.sender_type == "agent":
            out.append(AIMessage(content=m.content))
        else:
            out.append(HumanMessage(content=m.content))
    if extra_user_text:
        out.append(HumanMessage(content=extra_user_text))
    return out


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

    resolved = await resolve_all_mentions(db, group_id, content)
    if not resolved:
        return MessageSendResult(
            user_message=user_msg,
            agent_replies=[],
            warnings=["no agent mentioned in this group"],
        )

    graph = request.app.state.graph

    agent_replies: list[Message] = []
    for _, agent in resolved:
        chat_thread = await thread_service.get_or_create_chat_thread(
            db, group_id, agent.id, sender.id
        )
        await thread_service.mark_running(db, chat_thread)
        try:
            input_messages = await _build_lc_input(db, group, agent)
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
            agent_msg = await _persist_agent_message(
                db, group_id, agent, text, chat_thread.id, reply_to=user_msg.id
            )
            agent_replies.append(agent_msg)
            await thread_service.mark_completed(db, chat_thread)
        except Exception:
            await thread_service.mark_failed(db, chat_thread)
            raise

    return MessageSendResult(
        user_message=user_msg, agent_replies=agent_replies, warnings=[]
    )


async def _stream_one_agent(
    db: AsyncSession,
    graph: CompiledStateGraph[Any, Any, Any, Any],
    group: Group,
    agent: Agent,
    chat_thread: Thread,
    reply_to: UUID,
) -> AsyncIterator[dict[str, str]]:
    """Stream one agent's reply, persisting on graceful done OR on cancel.

    Yields token + agent_message events. Caller wraps multiple invocations
    for fan-out.
    """
    agent_id_str = str(agent.id)
    chunks: list[str] = []
    cancelled = False
    try:
        input_messages = await _build_lc_input(db, group, agent)
        chat_model = await resolve_chat_model(db, agent, streaming=True)
        async for kind, payload in runtime.run_with_stream(
            graph=graph,
            thread_id=str(chat_thread.id),
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
                final: AIMessage = payload
                text = (
                    final.content
                    if isinstance(final.content, str)
                    else "".join(chunks)
                )
                agent_msg = await _persist_agent_message(
                    db, group.id, agent, text, chat_thread.id, reply_to=reply_to
                )
                yield {
                    "event": "agent_message",
                    "data": json.dumps(_serialize_msg(agent_msg)),
                }
        await thread_service.mark_completed(db, chat_thread)
    except asyncio.CancelledError:
        cancelled = True
        partial = "".join(chunks)
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
    """Yield SSE events: user_message → (token×N + agent_message) per agent → done.

    Token events carry JSON `{"agent_id": ..., "delta": ...}` so the client
    knows which agent in the fan-out is currently speaking.

    On client disconnect mid-stream: the currently streaming agent's partial
    reply is persisted with `status='interrupted'`, the chat_thread is
    marked `paused`, and the rest of the fan-out (if any) is skipped.
    """
    group = await group_service.get_group(db, group_id, sender)
    user_msg = await _persist_user_message(db, group_id, sender, content)
    yield {"event": "user_message", "data": json.dumps(_serialize_msg(user_msg))}

    resolved = await resolve_all_mentions(db, group_id, content)
    if not resolved:
        yield {"event": "warning", "data": "no agent mentioned in this group"}
        yield {"event": "done", "data": ""}
        return

    graph = request.app.state.graph

    for _, agent in resolved:
        chat_thread = await thread_service.get_or_create_chat_thread(
            db, group_id, agent.id, sender.id
        )
        await thread_service.mark_running(db, chat_thread)
        async for event in _stream_one_agent(
            db, graph, group, agent, chat_thread, reply_to=user_msg.id
        ):
            yield event

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
            db, group, agent, extra_user_text=RESUME_CONTINUATION_PROMPT
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
) -> tuple[Thread, Agent, Group, Message]:
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
    agent = await db.scalar(select(Agent).where(Agent.id == thread.agent_id))
    if agent is None:
        raise NotFoundError(f"agent {thread.agent_id}")
    interrupted_msg = await _latest_interrupted_message(db, thread.id)
    if interrupted_msg is None:
        raise ConflictError(
            f"thread {thread_id} has no interrupted message to resume"
        )
    return thread, agent, group, interrupted_msg
