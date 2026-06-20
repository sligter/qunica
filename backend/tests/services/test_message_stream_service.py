import json
from collections.abc import AsyncIterator
from types import SimpleNamespace
from typing import cast
from uuid import uuid4

import pytest_asyncio
from fastapi import Request
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker, create_async_engine
from sqlalchemy.pool import StaticPool

from app.models.agent import Agent
from app.models.group import Group
from app.models.group_agent import GroupAgent
from app.models.group_member import GroupMember
from app.models.message import Message
from app.models.user import User
from app.models.workspace import Workspace
from app.services.message_service import send_message_stream


@pytest_asyncio.fixture
async def stream_session_factory() -> AsyncIterator[async_sessionmaker[AsyncSession]]:
    engine = create_async_engine(
        "sqlite+aiosqlite:///:memory:",
        connect_args={"check_same_thread": False},
        poolclass=StaticPool,
    )
    async with engine.begin() as conn:
        for table in (
            User.__table__,
            Workspace.__table__,
            Group.__table__,
            GroupMember.__table__,
            Agent.__table__,
            GroupAgent.__table__,
            Message.__table__,
        ):
            await conn.run_sync(table.create)

    session_factory = async_sessionmaker(
        engine,
        class_=AsyncSession,
        expire_on_commit=False,
    )
    yield session_factory
    await engine.dispose()


async def _seed_group(db: AsyncSession) -> tuple[User, Group]:
    user = User(
        email=f"stream-{uuid4().hex[:8]}@example.com",
        password_hash="x",
        name="Stream Owner",
    )
    db.add(user)
    await db.flush()

    workspace = Workspace(
        owner_id=user.id,
        name="Stream workspace",
        backend_type="local",
        local_path=".",
    )
    db.add(workspace)
    await db.flush()

    group = Group(
        owner_id=user.id,
        workspace_id=workspace.id,
        name="Stream group",
    )
    db.add(group)
    await db.flush()

    member = GroupMember(
        group_id=group.id,
        user_id=user.id,
        role="owner",
        status="active",
    )
    db.add(member)
    await db.commit()
    return user, group


def _request_stub() -> Request:
    return cast(
        Request,
        SimpleNamespace(app=SimpleNamespace(state=SimpleNamespace(graph=None))),
    )


async def test_stream_user_message_survives_request_rollback_after_emit(
    stream_session_factory: async_sessionmaker[AsyncSession],
) -> None:
    async with stream_session_factory() as db:
        user, group = await _seed_group(db)
        stream = send_message_stream(
            db,
            _request_stub(),
            group.id,
            user,
            "hello after stop all",
        )

        event = await anext(stream)
        assert event["event"] == "user_message"
        emitted = json.loads(event["data"])

        await stream.aclose()
        await db.rollback()

    async with stream_session_factory() as verify_db:
        messages = list(
            await verify_db.scalars(
                select(Message)
                .where(Message.group_id == group.id)
                .order_by(Message.created_at.asc(), Message.id.asc())
            )
        )

    assert [str(message.id) for message in messages] == [emitted["id"]]
    assert messages[0].content == "hello after stop all"
