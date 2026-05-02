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
"""

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
    SystemMessage,
)
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.agents import runtime
from app.agents.router import resolve_all_mentions
from app.models.agent import Agent
from app.models.group import Group
from app.models.message import Message
from app.models.user import User
from app.services import group_service, thread_service

CONTEXT_WINDOW = 20


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
        .where(Message.group_id == group_id, Message.status == "visible")
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
) -> Message:
    msg = Message(
        group_id=group_id,
        thread_id=thread_id,
        sender_type="agent",
        sender_id=agent.id,
        message_type="text",
        content=content,
        reply_to_message_id=reply_to,
    )
    db.add(msg)
    await db.flush()
    await db.refresh(msg)
    return msg


async def _build_lc_input(
    db: AsyncSession, group: Group, agent: Agent
) -> list[BaseMessage]:
    """Build the LangChain message list for an agent invocation.

    Includes the just-flushed current user message (it's already in the
    group history because `_persist_user_message` flushes before this is
    called).
    """
    system_parts: list[str] = [agent.system_prompt]
    if group.announcement:
        system_parts.append(f"Group announcement:\n{group.announcement}")
    system = "\n\n".join(system_parts)

    history_stmt = (
        select(Message)
        .where(Message.group_id == group.id, Message.status == "visible")
        .order_by(Message.created_at.desc())
        .limit(CONTEXT_WINDOW)
    )
    history = list(await db.scalars(history_stmt))
    history.reverse()

    out: list[BaseMessage] = [SystemMessage(content=system)]
    for m in history:
        if m.content is None:
            continue
        if m.sender_type == "agent":
            out.append(AIMessage(content=m.content))
        else:
            out.append(HumanMessage(content=m.content))
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
            response: AIMessage = await runtime.run(
                graph=graph,
                thread_id=str(chat_thread.id),
                llm_config=agent.llm_config,
                input_messages=input_messages,
            )
            text = response.content if isinstance(response.content, str) else str(response.content)
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
        agent_id_str = str(agent.id)
        try:
            input_messages = await _build_lc_input(db, group, agent)
            chunks: list[str] = []
            async for kind, payload in runtime.run_with_stream(
                graph=graph,
                thread_id=str(chat_thread.id),
                llm_config=agent.llm_config,
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
                    text = final.content if isinstance(final.content, str) else "".join(chunks)
                    agent_msg = await _persist_agent_message(
                        db, group_id, agent, text, chat_thread.id, reply_to=user_msg.id
                    )
                    yield {
                        "event": "agent_message",
                        "data": json.dumps(_serialize_msg(agent_msg)),
                    }
            await thread_service.mark_completed(db, chat_thread)
        except Exception:
            await thread_service.mark_failed(db, chat_thread)
            raise

    yield {"event": "done", "data": ""}
