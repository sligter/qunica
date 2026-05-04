from typing import Self

import pytest
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.security import hash_password
from app.models.agent import Agent
from app.models.group import Group
from app.models.group_agent import GroupAgent
from app.models.group_member import GroupMember
from app.models.message import Message
from app.models.thread import Thread
from app.models.user import User
from scripts import smoke_test


class _TestSessionFactory:
    def __init__(self, session: AsyncSession) -> None:
        self.session = session

    def __call__(self) -> Self:
        return self

    async def __aenter__(self) -> AsyncSession:
        return self.session

    async def __aexit__(self, *_exc: object) -> None:
        return None


async def test_smoke_cleanup_removes_generated_users_and_owned_records(
    db_session: AsyncSession,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    smoke_user = User(
        email="smoke-cleanup@example.com",
        name="Smoke Tester",
        password_hash=hash_password("test-password-123"),
    )
    scratch_user = User(
        email="x-deadbeef@example.com",
        name="X",
        password_hash=hash_password("test-password-123"),
    )
    real_user = User(
        email="real-smoke-cleanup@example.com",
        name="Smoke Real",
        password_hash=hash_password("test-password-123"),
    )
    db_session.add_all([smoke_user, scratch_user, real_user])
    await db_session.flush()

    agent = Agent(owner_id=smoke_user.id, name="Echo", system_prompt="Help")
    group = Group(owner_id=smoke_user.id, name="Smoke Project")
    db_session.add_all([agent, group])
    await db_session.flush()

    db_session.add_all(
        [
            GroupMember(group_id=group.id, user_id=smoke_user.id, role="owner"),
            GroupAgent(group_id=group.id, agent_id=agent.id),
            Thread(group_id=group.id, agent_id=agent.id, created_by=smoke_user.id),
            Message(
                group_id=group.id,
                sender_type="user",
                sender_id=smoke_user.id,
                message_type="text",
                content="smoke",
            ),
        ]
    )
    await db_session.flush()

    monkeypatch.setattr(smoke_test, "SessionLocal", _TestSessionFactory(db_session))

    removed = await smoke_test.delete_generated_smoke_users(smoke_user.email)

    assert removed == 2
    assert await db_session.scalar(select(User).where(User.id == smoke_user.id)) is None
    assert await db_session.scalar(select(User).where(User.id == scratch_user.id)) is None
    assert await db_session.scalar(select(Agent).where(Agent.id == agent.id)) is None
    assert await db_session.scalar(select(Group).where(Group.id == group.id)) is None
    assert await db_session.scalar(select(User).where(User.id == real_user.id)) is not None
