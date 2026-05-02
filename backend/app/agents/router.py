"""Mention parsing & resolution.

Phase 1 Week 3-4 expanded API:
- `parse_mention_tokens(text)`: ordered list of @-tokens.
- `resolve_first_mention(...)`: the first @-token that matches an agent in
  the group. Kept for backward compatibility; new code should prefer
  `resolve_all_mentions`.
- `resolve_all_mentions(...)`: ALL matching agents in textual order,
  deduplicated by `agent_id` (a duplicate mention of the same agent in one
  message triggers it once). Used by the multi-agent fan-out flow in
  `message_service.send_message[_stream]`.

This file will be replaced by a LangGraph router node when richer routing
strategies (e.g., implicit response by keyword, broadcast to all) land.
"""

from __future__ import annotations

import re
from uuid import UUID

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.models.agent import Agent
from app.models.group_agent import GroupAgent

# Allow ASCII word chars, CJK, and hyphens in mention tokens.
_MENTION_RE = re.compile(r"@([\w一-鿿\-]+)")


def parse_mention_tokens(text: str) -> list[str]:
    return _MENTION_RE.findall(text)


async def _agents_by_name(
    db: AsyncSession, group_id: UUID
) -> dict[str, tuple[GroupAgent, Agent]]:
    """All active group_agents indexed by lowercased effective display name."""
    stmt = (
        select(GroupAgent, Agent)
        .join(Agent, Agent.id == GroupAgent.agent_id)
        .where(GroupAgent.group_id == group_id, GroupAgent.status == "active")
    )
    rows = (await db.execute(stmt)).all()
    by_name: dict[str, tuple[GroupAgent, Agent]] = {}
    for ga, agent in rows:
        ga_obj: GroupAgent = ga
        agent_obj: Agent = agent
        effective = (ga_obj.display_name or agent_obj.name).lower()
        by_name.setdefault(effective, (ga_obj, agent_obj))
    return by_name


async def resolve_first_mention(
    db: AsyncSession, group_id: UUID, text: str
) -> tuple[GroupAgent, Agent] | None:
    tokens = parse_mention_tokens(text)
    if not tokens:
        return None
    by_name = await _agents_by_name(db, group_id)
    if not by_name:
        return None
    for token in tokens:
        match = by_name.get(token.lower())
        if match is not None:
            return match
    return None


async def resolve_all_mentions(
    db: AsyncSession, group_id: UUID, text: str
) -> list[tuple[GroupAgent, Agent]]:
    """All matching agents in textual order, deduplicated by agent_id.

    Unmatched @-tokens are silently dropped. The order matters because the
    fan-out runs sequentially; later agents see earlier agents' replies via
    the rolling group history window.
    """
    tokens = parse_mention_tokens(text)
    if not tokens:
        return []
    by_name = await _agents_by_name(db, group_id)
    if not by_name:
        return []

    out: list[tuple[GroupAgent, Agent]] = []
    seen_agent_ids: set[UUID] = set()
    for token in tokens:
        match = by_name.get(token.lower())
        if match is None:
            continue
        _, agent = match
        if agent.id in seen_agent_ids:
            continue
        seen_agent_ids.add(agent.id)
        out.append(match)
    return out
