"""Mention parsing & resolution.

Phase 1 Week 3-4 expanded API:
- `parse_mention_tokens(text)`: ordered list of @-tokens (single-word
  heuristic; coarse — see note below).
- `resolve_first_mention(...)`: the first @-token that matches an agent in
  the group. Kept for backward compatibility.
- `resolve_all_mentions(...)`: ALL matching agents in textual order,
  deduplicated by `agent_id`. Used by the multi-agent fan-out flow in
  `message_service.send_message[_stream]`.

Resolution algorithm (handles agent display names containing spaces):
walk the text left-to-right; at every '@' position, try the longest known
agent display name as a prefix of the substring after '@'. The matched
name must be followed by a non-name character or end-of-string so that
e.g. `@echolike` does NOT match an agent called `echo`.

This file will be replaced by a LangGraph router node when richer routing
strategies (e.g., implicit response by keyword, broadcast to all) land.
"""

from __future__ import annotations

import re
from uuid import UUID

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.models.agent import Agent
from app.models.group import Group
from app.models.group_agent import GroupAgent

# Coarse single-word heuristic. Does NOT understand multi-word display names;
# kept for ad-hoc "are there any @-style tokens at all" checks. The
# authoritative resolution happens in `resolve_all_mentions`.
_MENTION_RE = re.compile(r"@([\w一-鿿\-]+)")


def parse_mention_tokens(text: str) -> list[str]:
    """Coarse @-token extractor (single-word, no spaces).

    For real resolution against the group roster, use
    `resolve_all_mentions` — it handles multi-word display names and
    longest-match disambiguation.
    """
    return _MENTION_RE.findall(text)


def _is_name_char(ch: str) -> bool:
    """True if `ch` could be part of an agent display name token.

    Used as the boundary check after a longest-match prefix to prevent
    `@echolike` from matching agent `echo`. Spaces are deliberately
    excluded — a space ends one mention candidate; the next mention
    starts at the next `@`.
    """
    if ch.isalnum():
        return True
    if ch in "_-":
        return True
    return "一" <= ch <= "鿿"


async def _candidate_agents(
    db: AsyncSession, group: Group
) -> list[tuple[str, tuple[GroupAgent, Agent]]]:
    """Active unmuted (display_name_lower, (group_agent, agent)) pairs, longest first.

    First-match-wins is enforced when two agents share the same effective
    display name (rare; the API allows it but the UI discourages).
    """
    muted_agent_ids = set(group.muted_agent_ids or [])
    stmt = (
        select(GroupAgent, Agent)
        .join(Agent, Agent.id == GroupAgent.agent_id)
        .where(GroupAgent.group_id == group.id, GroupAgent.status == "active")
    )
    rows = (await db.execute(stmt.order_by(GroupAgent.joined_at.asc()))).all()
    seen_names: set[str] = set()
    candidates: list[tuple[str, tuple[GroupAgent, Agent]]] = []
    for ga, agent in rows:
        if str(agent.id) in muted_agent_ids:
            continue
        name = (ga.display_name or agent.name).lower()
        if name in seen_names:
            continue
        seen_names.add(name)
        candidates.append((name, (ga, agent)))
    candidates.sort(key=lambda kv: -len(kv[0]))
    return candidates


def _scan_mentions(
    text: str, candidates: list[tuple[str, tuple[GroupAgent, Agent]]]
) -> list[tuple[GroupAgent, Agent]]:
    """Walk `text`, longest-match each `@<name>` against `candidates`."""
    if not candidates:
        return []
    out: list[tuple[GroupAgent, Agent]] = []
    seen_agent_ids: set[UUID] = set()
    lower = text.lower()
    n = len(text)
    i = 0
    while i < n:
        if text[i] != "@":
            i += 1
            continue
        rest = lower[i + 1 :]
        matched = False
        for name, pair in candidates:
            if not rest.startswith(name):
                continue
            end = i + 1 + len(name)
            if end != n and _is_name_char(text[end]):
                continue
            if pair[1].id not in seen_agent_ids:
                seen_agent_ids.add(pair[1].id)
                out.append(pair)
            i = end
            matched = True
            break
        if not matched:
            i += 1
    return out


async def resolve_all_mentions(
    db: AsyncSession, group: Group, text: str
) -> list[tuple[GroupAgent, Agent]]:
    """Determine which agents should respond to a message.

    Routing rules (evaluated in order):
    1. Explicit @-mentions always take priority: only the mentioned agents
       respond, regardless of free_speech mode.
    2. If no explicit @-mentions AND `group.free_speech` is True, ALL
       active unmuted agents respond (joined-at order).
    3. If no explicit @-mentions AND free_speech is off, no agents respond.

    Muted agents are always skipped. The returned order matters because the
    fan-out runs sequentially; later agents see earlier agents' replies via
    the rolling group history window.
    """
    candidates = await _candidate_agents(db, group)
    if not candidates:
        return []

    # Resolve explicit @-mentions first (in textual order, deduped)
    if "@" in text:
        mentioned = _scan_mentions(text, candidates)
        if mentioned:
            return mentioned

    # No explicit @-mentions: free_speech → all agents; otherwise none
    if group.free_speech:
        joined_order = sorted(candidates, key=lambda item: (item[1][0].joined_at, item[1][0].id))
        return [(ga, agent) for _name, (ga, agent) in joined_order]

    return []


async def resolve_first_mention(
    db: AsyncSession, group: Group, text: str
) -> tuple[GroupAgent, Agent] | None:
    matches = await resolve_all_mentions(db, group, text)
    return matches[0] if matches else None
