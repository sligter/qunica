from __future__ import annotations

import os
from pathlib import Path
from uuid import uuid4

import aiosqlite
import pytest
from sqlalchemy import select
from sqlalchemy.ext.asyncio import async_sessionmaker, create_async_engine

from app.agents.runtime import compile_graph
from app.desktop_server import _configure_env
from app.main import _apply_sqlite_schema_patches
from app.models import Agent, Base
from app.models.workspace import Workspace


@pytest.mark.asyncio
async def test_sqlite_metadata_round_trips_uuid_and_json() -> None:
    engine = create_async_engine("sqlite+aiosqlite:///:memory:")
    try:
        async with engine.begin() as conn:
            await conn.run_sync(Base.metadata.create_all)
        session_factory = async_sessionmaker(engine, expire_on_commit=False)
        owner_id = uuid4()
        async with session_factory() as session:
            workspace = Workspace(
                owner_id=owner_id,
                name="Local",
                backend_type="local",
                local_path="/tmp/project",
            )
            session.add(workspace)
            await session.flush()
            agent = Agent(
                owner_id=owner_id,
                name="Codex",
                system_prompt="Work carefully",
                workspace_id=workspace.id,
                runtime_kind="external_cli",
                external_runtime={"adapter": "codex", "timeout_seconds": 3600},
                tool_config={"tools": {}, "assistant_agents": []},
                skill_ids=[],
            )
            session.add(agent)
            await session.commit()
            loaded = await session.scalar(select(Agent).where(Agent.id == agent.id))
        assert loaded is not None
        assert loaded.owner_id == owner_id
        assert loaded.external_runtime == {"adapter": "codex", "timeout_seconds": 3600}
    finally:
        await engine.dispose()


@pytest.mark.asyncio
async def test_sqlite_schema_patch_adds_agent_free_mention_limit() -> None:
    engine = create_async_engine("sqlite+aiosqlite:///:memory:")
    try:
        async with engine.begin() as conn:
            await conn.exec_driver_sql("CREATE TABLE groups (id CHAR(36) PRIMARY KEY)")

            await _apply_sqlite_schema_patches(conn)
            await _apply_sqlite_schema_patches(conn)

            columns_result = await conn.exec_driver_sql("PRAGMA table_info(groups)")
            columns = {str(row[1]): row for row in columns_result.fetchall()}
            assert "agent_free_mention_max_dispatches" in columns

            await conn.exec_driver_sql("INSERT INTO groups (id) VALUES ('legacy-group')")
            value_result = await conn.exec_driver_sql(
                "SELECT agent_free_mention_max_dispatches FROM groups WHERE id = 'legacy-group'"
            )
            assert value_result.scalar_one() == 8
    finally:
        await engine.dispose()


def test_desktop_server_sets_sqlite_urls(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("DATABASE_URL", raising=False)
    monkeypatch.delenv("CHECKPOINT_DATABASE_URL", raising=False)
    monkeypatch.delenv("DESKTOP_APP_DATA_DIR", raising=False)

    _configure_env(tmp_path, port=8765)

    assert os.environ["DESKTOP_APP_DATA_DIR"] == str(tmp_path)
    assert os.environ["DATABASE_URL"].startswith("sqlite+aiosqlite:///")
    assert os.environ["DATABASE_URL"].endswith("/ag-swarmer.sqlite3")
    assert os.environ["CHECKPOINT_DATABASE_URL"].endswith("/langgraph-checkpoints.sqlite3")


@pytest.mark.asyncio
async def test_async_sqlite_saver_compiles_langgraph(tmp_path: Path) -> None:
    from langgraph.checkpoint.sqlite.aio import AsyncSqliteSaver

    async with aiosqlite.connect(tmp_path / "checkpoints.sqlite3") as conn:
        saver = AsyncSqliteSaver(conn)
        await saver.setup()
        graph = compile_graph(saver)

    assert graph is not None
