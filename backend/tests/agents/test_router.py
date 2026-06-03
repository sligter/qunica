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
    group, _ = await _seed_group_with_agents(db_session, ["Echo", "Mirror"])
    result = await resolve_first_mention(db_session, group, "@Mirror @Echo")
    assert result is not None
    _, agent = result
    assert agent.name == "Mirror"


@pytest.mark.asyncio
async def test_resolve_first_mention_returns_none_when_unmatched(
    db_session: AsyncSession,
) -> None:
    group, _ = await _seed_group_with_agents(db_session, ["Echo"])
    result = await resolve_first_mention(db_session, group, "@NotInGroup hi")
    assert result is None


@pytest.mark.asyncio
async def test_resolve_all_mentions_preserves_textual_order(
    db_session: AsyncSession,
) -> None:
    group, _ = await _seed_group_with_agents(db_session, ["Echo", "Mirror", "Nova"])
    result = await resolve_all_mentions(db_session, group, "@Nova @Echo @Mirror go")
    names = [a.name for _, a in result]
    assert names == ["Nova", "Echo", "Mirror"]


@pytest.mark.asyncio
async def test_resolve_all_mentions_dedupes_repeated_agent(
    db_session: AsyncSession,
) -> None:
    group, _ = await _seed_group_with_agents(db_session, ["Echo"])
    result = await resolve_all_mentions(db_session, group, "@Echo @Echo @Echo")
    assert len(result) == 1
    assert result[0][1].name == "Echo"


@pytest.mark.asyncio
async def test_resolve_all_mentions_drops_unmatched_silently(
    db_session: AsyncSession,
) -> None:
    group, _ = await _seed_group_with_agents(db_session, ["Echo"])
    result = await resolve_all_mentions(
        db_session, group, "@NotInGroup @Echo @AlsoMissing"
    )
    assert len(result) == 1
    assert result[0][1].name == "Echo"


@pytest.mark.asyncio
async def test_resolve_all_mentions_skips_muted_agents(
    db_session: AsyncSession,
) -> None:
    group, agents = await _seed_group_with_agents(db_session, ["Echo", "Mirror"])
    group.muted_agent_ids = [str(agents[0].id)]
    await db_session.flush()

    result = await resolve_all_mentions(db_session, group, "@Echo @Mirror")

    assert [agent.name for _, agent in result] == ["Mirror"]


@pytest.mark.asyncio
async def test_resolve_first_mention_skips_muted_agents(
    db_session: AsyncSession,
) -> None:
    group, agents = await _seed_group_with_agents(db_session, ["Echo", "Mirror"])
    group.muted_agent_ids = [str(agents[0].id)]
    await db_session.flush()

    result = await resolve_first_mention(db_session, group, "@Echo @Mirror")

    assert result is not None
    assert result[1].name == "Mirror"


@pytest.mark.asyncio
async def test_resolve_all_mentions_case_insensitive(
    db_session: AsyncSession,
) -> None:
    group, _ = await _seed_group_with_agents(db_session, ["Echo"])
    result = await resolve_all_mentions(db_session, group, "@ECHO say hi")
    assert len(result) == 1


@pytest.mark.asyncio
async def test_resolve_all_mentions_with_spaces_in_name(
    db_session: AsyncSession,
) -> None:
    """Regression test for the @tree man bug — display names with spaces."""
    group, _ = await _seed_group_with_agents(db_session, ["Echo", "tree man"])
    result = await resolve_all_mentions(
        db_session, group, "@Echo @tree man say something brief."
    )
    names = [a.name for _, a in result]
    assert names == ["Echo", "tree man"]


@pytest.mark.asyncio
async def test_resolve_all_mentions_longest_match_wins(
    db_session: AsyncSession,
) -> None:
    """Two agents `tree` and `tree man`: @tree man picks `tree man` (longer)."""
    group, _ = await _seed_group_with_agents(db_session, ["tree", "tree man"])
    result = await resolve_all_mentions(db_session, group, "@tree man hi")
    assert [a.name for _, a in result] == ["tree man"]


@pytest.mark.asyncio
async def test_resolve_all_mentions_short_name_still_matches(
    db_session: AsyncSession,
) -> None:
    """Same setup as above but `@tree x` — only `tree` matches because `man` is missing."""
    group, _ = await _seed_group_with_agents(db_session, ["tree", "tree man"])
    result = await resolve_all_mentions(db_session, group, "@tree x")
    assert [a.name for _, a in result] == ["tree"]


@pytest.mark.asyncio
async def test_resolve_all_mentions_no_substring_match(
    db_session: AsyncSession,
) -> None:
    """`@echolike` does NOT match agent `Echo` (boundary check)."""
    group, _ = await _seed_group_with_agents(db_session, ["Echo"])
    result = await resolve_all_mentions(db_session, group, "@echolike hi")
    assert result == []


@pytest.mark.asyncio
async def test_resolve_all_mentions_trailing_punctuation(
    db_session: AsyncSession,
) -> None:
    """`@Echo,` should match Echo (comma is a non-name boundary)."""
    group, _ = await _seed_group_with_agents(db_session, ["Echo"])
    result = await resolve_all_mentions(db_session, group, "@Echo, please reply")
    assert [a.name for _, a in result] == ["Echo"]


@pytest.mark.asyncio
async def test_resolve_all_mentions_dedupes_multiword_too(
    db_session: AsyncSession,
) -> None:
    group, _ = await _seed_group_with_agents(db_session, ["tree man"])
    result = await resolve_all_mentions(
        db_session, group, "@tree man hi @tree man again"
    )
    assert len(result) == 1
    assert result[0][1].name == "tree man"


@pytest.mark.asyncio
async def test_free_speech_all_agents_respond_without_mention(
    db_session: AsyncSession,
) -> None:
    """free_speech=True → all agents respond even without @mentions."""
    group, _ = await _seed_group_with_agents(db_session, ["Echo", "Mirror", "Nova"])
    group.free_speech = True
    await db_session.flush()

    result = await resolve_all_mentions(db_session, group, "hello everyone")
    names = [a.name for _, a in result]
    assert len(names) == 3
    assert set(names) == {"Echo", "Mirror", "Nova"}


@pytest.mark.asyncio
async def test_free_speech_with_mention_only_mentioned_respond(
    db_session: AsyncSession,
) -> None:
    """free_speech=True + explicit @mention → only mentioned agents respond."""
    group, _ = await _seed_group_with_agents(db_session, ["Echo", "Mirror", "Nova"])
    group.free_speech = True
    await db_session.flush()

    result = await resolve_all_mentions(db_session, group, "@Nova what do you think?")
    names = [a.name for _, a in result]
    assert names == ["Nova"]


@pytest.mark.asyncio
async def test_free_speech_skips_muted_agents(
    db_session: AsyncSession,
) -> None:
    """free_speech=True still respects mute list."""
    group, agents = await _seed_group_with_agents(db_session, ["Echo", "Mirror"])
    group.free_speech = True
    group.muted_agent_ids = [str(agents[0].id)]
    await db_session.flush()

    result = await resolve_all_mentions(db_session, group, "hi all")
    assert [a.name for _, a in result] == ["Mirror"]


@pytest.mark.asyncio
async def test_resolve_all_mentions_skips_deleted_agents(
    db_session: AsyncSession,
) -> None:
    group, agents = await _seed_group_with_agents(db_session, ["Echo", "Mirror"])
    agents[0].status = "deleted"
    group.free_speech = True
    await db_session.flush()

    mentioned = await resolve_all_mentions(db_session, group, "@Echo @Mirror")
    broadcast = await resolve_all_mentions(db_session, group, "hi all")

    assert [agent.name for _, agent in mentioned] == ["Mirror"]
    assert [agent.name for _, agent in broadcast] == ["Mirror"]


@pytest.mark.asyncio
async def test_no_mention_no_free_speech_returns_empty(
    db_session: AsyncSession,
) -> None:
    """free_speech=False + no @mention → empty list."""
    group, _ = await _seed_group_with_agents(db_session, ["Echo", "Mirror"])

    result = await resolve_all_mentions(db_session, group, "hello everyone")
    assert result == []
