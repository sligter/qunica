"""Shared pytest fixtures.

Test isolation strategy:
- One session-scoped event loop for everything (asyncpg + psycopg both
  require the SelectorEventLoop on Windows, and the AsyncPostgresSaver +
  app.db.engine connections must live in the same loop they were created
  in).
- Lifespan startup runs once via `asgi-lifespan.LifespanManager`, so
  `app.state.graph` is populated for the message endpoints.
- Per-test outer transaction with a SAVEPOINT-bound AsyncSession, rolled
  back at teardown. The endpoint's `get_db` is overridden to share that
  session, so all writes are wiped between tests.
- Fake LLM via monkey-patch on every `make_chat_model` import site.
"""

from __future__ import annotations

import asyncio
import sys

# Both psycopg (langgraph) and asyncpg (sqlalchemy) need SelectorEventLoop
# on Windows. Set policy before any asyncio event loop is created.
if sys.platform == "win32":
    asyncio.set_event_loop_policy(asyncio.WindowsSelectorEventLoopPolicy())

import secrets  # noqa: E402
from collections.abc import AsyncIterator, Iterator  # noqa: E402
from typing import Any  # noqa: E402

import pytest  # noqa: E402
import pytest_asyncio  # noqa: E402
from asgi_lifespan import LifespanManager  # noqa: E402
from httpx import ASGITransport, AsyncClient  # noqa: E402
from langchain_core.language_models.fake_chat_models import GenericFakeChatModel  # noqa: E402
from langchain_core.messages import AIMessage  # noqa: E402
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker  # noqa: E402

from app.db import engine, get_db  # noqa: E402
from app.main import app  # noqa: E402


@pytest_asyncio.fixture(scope="session")
async def _lifespan() -> AsyncIterator[None]:
    """Run the FastAPI lifespan once for the whole test session so that
    `app.state.graph` is populated and the AsyncPostgresSaver tables exist.
    """
    async with LifespanManager(app):
        yield


@pytest_asyncio.fixture
async def db_session(_lifespan: None) -> AsyncIterator[AsyncSession]:
    """Per-test transaction, rolled back at teardown."""
    async with engine.connect() as conn:
        outer = await conn.begin()
        session_factory = async_sessionmaker(
            bind=conn,
            class_=AsyncSession,
            expire_on_commit=False,
            join_transaction_mode="create_savepoint",
        )
        async with session_factory() as session:
            try:
                yield session
            finally:
                await session.close()
        if outer.is_active:
            await outer.rollback()


@pytest_asyncio.fixture
async def client(db_session: AsyncSession) -> AsyncIterator[AsyncClient]:
    """HTTP client with `get_db` overridden to share the test's session."""

    async def _override_get_db() -> AsyncIterator[AsyncSession]:
        # Don't commit/rollback here — the outer fixture owns the boundary.
        yield db_session

    app.dependency_overrides[get_db] = _override_get_db
    transport = ASGITransport(app=app)
    try:
        async with AsyncClient(transport=transport, base_url="http://test") as ac:
            yield ac
    finally:
        app.dependency_overrides.pop(get_db, None)


def _make_fake_chat_model(messages: list[str] | None = None) -> Any:
    payload = messages or ["FAKE-REPLY"]
    return GenericFakeChatModel(messages=iter([AIMessage(content=m) for m in payload]))


@pytest.fixture(autouse=True)
def fake_llm(monkeypatch: pytest.MonkeyPatch) -> Iterator[dict[str, Any]]:
    """Patch every `make_chat_model` import site with a deterministic fake.

    Tests that need a custom script can mutate `state["messages"]` before
    triggering the LLM call.
    """
    state: dict[str, list[str]] = {"messages": ["FAKE-REPLY"]}

    def _factory(_llm_config: dict[str, Any] | None = None) -> Any:
        return _make_fake_chat_model(list(state["messages"]))

    monkeypatch.setattr("app.llm.chat_model.make_chat_model", _factory)
    monkeypatch.setattr("app.api.v1.agents.make_chat_model", _factory)
    monkeypatch.setattr("app.agents.runtime.make_chat_model", _factory)
    yield state


@pytest_asyncio.fixture
async def auth_headers(client: AsyncClient) -> dict[str, str]:
    """Register a fresh user and return Authorization headers."""
    suffix = secrets.token_hex(4)
    email = f"test-{suffix}@example.com"
    password = "test-password-123"
    r = await client.post(
        "/api/v1/auth/register",
        json={"email": email, "password": password, "name": f"Test {suffix}"},
    )
    assert r.status_code == 201, r.text
    r = await client.post(
        "/api/v1/auth/login", json={"email": email, "password": password}
    )
    assert r.status_code == 200, r.text
    token = r.json()["access_token"]
    return {"Authorization": f"Bearer {token}"}
