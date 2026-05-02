"""Mention parser unit tests — pure functions and DB-touching resolvers.

The DB-touching tests use the savepoint `db_session` fixture directly, no
HTTP client needed.
"""

from uuid import uuid4

import pytest
from sqlalchemy.ext.asyncio import AsyncSession

from app.agents.router import (
    parse_mention_tokens,
    resolve_all_mentions,
    resolve_first_mention,
)
from app.models.agent import Agent
from app.models.group import Group
from app.models.group_agent import GroupAgent
from app.models.group_member import GroupMember
from app.models.user import User


def test_parse_mention_tokens_ascii() -> None:
    assert parse_mention_tokens("hi @Echo @Mirror") == ["Echo", "Mirror"]


def test_parse_mention_tokens_cjk() -> None:
    assert parse_mention_tokens("你好 @小米 @Echo @数据助手") == ["小米", "Echo", "数据助手"]


def test_parse_mention_tokens_hyphen() -> None:
    assert parse_mention_tokens("@code-reviewer please") == ["code-reviewer"]


def test_parse_mention_tokens_empty() -> None:
    assert parse_mention_tokens("plain text no at-sign") == []


async def _seed_group_with_agents(
    db: AsyncSession, agent_names: list[str]
) -> tuple[Group, list[Agent]]:
    user = User(email=f"u-{uuid4().hex[:6]}@x", password_hash="x", name="U")
    db.add(user)
    await db.flush()

    group = Group(owner_id=user.id, name="G")
    db.add(group)
    await db.flush()

    db.add(GroupMember(group_id=group.id, user_id=user.id, role="owner"))

    agents = []
    for name in agent_names:
        a = Agent(
            owner_id=user.id, name=name, system_prompt="s"
        )
        db.add(a)
        await db.flush()
        db.add(GroupAgent(group_id=group.id, agent_id=a.id, response_mode="mentioned_only"))
        agents.append(a)

    await db.flush()
    return group, agents


@pytest.mark.asyncio
async def test_resolve_first_mention_returns_first_match(
    db_session: AsyncSession,
) -> None:
    group, agents = await _seed_group_with_agents(db_session, ["Echo", "Mirror"])
    result = await resolve_first_mention(db_session, group.id, "@Mirror @Echo")
    assert result is not None
    _, agent = result
    assert agent.name == "Mirror"


@pytest.mark.asyncio
async def test_resolve_first_mention_returns_none_when_unmatched(
    db_session: AsyncSession,
) -> None:
    group, _ = await _seed_group_with_agents(db_session, ["Echo"])
    result = await resolve_first_mention(db_session, group.id, "@NotInGroup hi")
    assert result is None


@pytest.mark.asyncio
async def test_resolve_all_mentions_preserves_textual_order(
    db_session: AsyncSession,
) -> None:
    group, agents = await _seed_group_with_agents(db_session, ["Echo", "Mirror", "Nova"])
    result = await resolve_all_mentions(db_session, group.id, "@Nova @Echo @Mirror go")
    names = [a.name for _, a in result]
    assert names == ["Nova", "Echo", "Mirror"]


@pytest.mark.asyncio
async def test_resolve_all_mentions_dedupes_repeated_agent(
    db_session: AsyncSession,
) -> None:
    group, _ = await _seed_group_with_agents(db_session, ["Echo"])
    result = await resolve_all_mentions(db_session, group.id, "@Echo @Echo @Echo")
    assert len(result) == 1
    assert result[0][1].name == "Echo"


@pytest.mark.asyncio
async def test_resolve_all_mentions_drops_unmatched_silently(
    db_session: AsyncSession,
) -> None:
    group, _ = await _seed_group_with_agents(db_session, ["Echo"])
    result = await resolve_all_mentions(
        db_session, group.id, "@NotInGroup @Echo @AlsoMissing"
    )
    assert len(result) == 1
    assert result[0][1].name == "Echo"


@pytest.mark.asyncio
async def test_resolve_all_mentions_case_insensitive(
    db_session: AsyncSession,
) -> None:
    group, _ = await _seed_group_with_agents(db_session, ["Echo"])
    result = await resolve_all_mentions(db_session, group.id, "@ECHO say hi")
    assert len(result) == 1
