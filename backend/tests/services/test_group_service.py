from collections.abc import AsyncIterator
from pathlib import Path
from uuid import uuid4

import pytest_asyncio
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker, create_async_engine
from sqlalchemy.pool import StaticPool

from app.models.group import Group
from app.models.group_member import GroupMember
from app.models.user import User
from app.models.workspace import Workspace
from app.schemas.group import GroupCreate, GroupUpdate
from app.services import group_service


@pytest_asyncio.fixture
async def group_db_session() -> AsyncIterator[AsyncSession]:
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
        ):
            await conn.run_sync(table.create)
    session_factory = async_sessionmaker(
        engine,
        class_=AsyncSession,
        expire_on_commit=False,
    )
    async with session_factory() as session:
        yield session
    await engine.dispose()


async def _user(db: AsyncSession) -> User:
    user = User(
        email=f"group-{uuid4().hex[:8]}@example.com",
        password_hash="x",
        name="Group Owner",
    )
    db.add(user)
    await db.flush()
    return user


async def _workspace(db: AsyncSession, owner: User, path: Path, name: str) -> Workspace:
    path.mkdir(parents=True, exist_ok=True)
    workspace = Workspace(
        owner_id=owner.id,
        name=name,
        backend_type="local",
        local_path=str(path),
    )
    db.add(workspace)
    await db.flush()
    return workspace


async def test_create_and_update_group_with_custom_workspaces(
    group_db_session: AsyncSession,
    tmp_path: Path,
) -> None:
    owner = await _user(group_db_session)
    first = await _workspace(group_db_session, owner, tmp_path / "first", "First")
    second = await _workspace(group_db_session, owner, tmp_path / "second", "Second")

    group = await group_service.create_group(
        group_db_session,
        GroupCreate(name="Custom workspace group", workspace_id=first.id),
        owner,
    )

    assert group.workspace_id == first.id

    updated = await group_service.update_group(
        group_db_session,
        group.id,
        GroupUpdate(workspace_id=second.id),
        owner,
    )

    assert updated.workspace_id == second.id
