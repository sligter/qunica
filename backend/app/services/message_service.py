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
from sqlalchemy import select, tuple_
from sqlalchemy.ext.asyncio import AsyncSession

from app.agents import runtime
from app.agents.context import (
    DEFAULT_RUNTIME_LIMITS,
    AgentInvocationContext,
    build_agent_invocation_context,
)
from app.agents.router import resolve_all_mentions
from app.agents.runtime import RuntimeAgentHandoff, RuntimeToolEvent, RuntimeWaitForUser
from app.agents.workspace_tools import build_workspace_tools
from app.core.exceptions import AgentChatError, ConflictError, NotFoundError
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
class MessageSendResult:
    user_message: Message
    agent_replies: list[Message]
    warnings: list[str]
    silent_turns: list[SilentAgentTurn]
    dispatch_messages: list[Message]
    all_silent: bool
    waiting_for_user: bool = False


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


async def _build_invocation(
    db: AsyncSession,
    group: Group,
    group_agent: GroupAgent,
    agent: Agent,
    extra_user_text: str | None = None,
    history_statuses: tuple[str, ...] = ("visible", "interrupted"),
) -> tuple[list[BaseMessage], AgentInvocationContext]:
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
    context = await build_agent_invocation_context(
        db,
        agent,
        owner,
        group=group,
        group_agent=group_agent,
        runtime_limits={**DEFAULT_RUNTIME_LIMITS, "context_history_messages": CONTEXT_WINDOW},
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
    return out, context


async def _build_lc_input(
    db: AsyncSession,
    group: Group,
    group_agent: GroupAgent,
    agent: Agent,
    extra_user_text: str | None = None,
) -> list[BaseMessage]:
    messages, _context = await _build_invocation(
        db, group, group_agent, agent, extra_user_text=extra_user_text
    )
    return messages


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


def _is_waiting_for_user_response(response: AIMessage) -> bool:
    return bool(response.additional_kwargs.get("waiting_for_user"))


def _is_agent_handoff_response(response: AIMessage) -> bool:
    return bool(response.additional_kwargs.get("agent_handoff"))


def _waiting_message_from_response(response: AIMessage) -> str:
    message = response.additional_kwargs.get("waiting_message")
    return str(message) if message else WAITING_FOR_USER_WARNING


def _tool_event_waits_for_user(tool_event: RuntimeToolEvent) -> bool:
    return tool_event.tool_name == "AskUser" and tool_event.status == "input_required"


def _serialize_tool_event(
    tool_event: RuntimeToolEvent, agent: Agent, group_agent: GroupAgent
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
                input_messages, context = await _build_invocation(db, group, group_agent, agent)
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
                        db, group_id, agent, visible_text, chat_thread.id, reply_to=user_msg.id
                    )
                    agent_replies.append(agent_msg)
                    visible_replies_used += 1
                    spoke_this_round = True
                    last_visible_agent_id = agent.id
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
        if waiting_for_user or handoff_dispatched or not group.proactive_mode:
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
            input_messages, context = await _build_invocation(
                db,
                group,
                dispatch.helper_group_agent,
                dispatch.helper_agent,
            )
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
        input_messages, context = await _build_invocation(db, group, group_agent, agent)
        chat_model = await resolve_chat_model(db, agent, streaming=True)
        async def _agent_tool_executor(
            agent_id: str, task: str, instructions: str | None = None
        ) -> str:
            if pending_dispatches is None or dispatch_counter is None:
                raise AgentChatError("group context is required for AgentAsTool dispatch")
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
                    "data": json.dumps(_serialize_tool_event(payload, agent, group_agent)),
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
                        {"agent_id": agent_id_str, "delta": delta}
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
                    if _is_agent_handoff_response(final):
                        if emitted_visible_len:
                            yield {
                                "event": "agent_silent",
                                "data": json.dumps(_agent_identity_payload(agent, group_agent)),
                            }
                        yield {"event": "agent_handoff", "data": ""}
                        continue
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
                    if visible_text:
                        agent_msg = await _persist_agent_message(
                            db, group.id, agent, visible_text, chat_thread.id, reply_to=reply_to
                        )
                        yield {
                            "event": "agent_message",
                            "data": json.dumps(_serialize_msg(agent_msg)),
                        }
                    if _is_waiting_for_user_response(final) or _requests_human_input(
                        visible_text, human_names, sender_name
                    ):
                        yield {
                            "event": "waiting_for_user",
                            "data": json.dumps({"message": _waiting_message_from_response(final)}),
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
    handoff_dispatched = False
    pending_dispatches: list[AgentToolDispatch] = []
    dispatch_counter = [0]

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
                    pending_dispatches=pending_dispatches,
                    dispatch_counter=dispatch_counter,
                ):
                    if event["event"] == "agent_message":
                        emitted_agent_messages += 1
                        spoke_this_round = True
                        last_visible_agent_id = agent.id
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
                    }),
                }
            if waiting_for_user or handoff_dispatched:
                break
        if waiting_for_user or handoff_dispatched or not group.proactive_mode:
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
                }),
            }

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
        input_messages, context = await _build_invocation(
            db, group, group_agent, agent, extra_user_text=RESUME_CONTINUATION_PROMPT
        )
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
