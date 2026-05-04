import secrets
from pathlib import Path
from typing import Any, cast
from uuid import UUID, uuid4

from httpx import AsyncClient
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.models.message import Message
from app.models.thread import Thread


async def _setup_group_with_thread(
    client: AsyncClient, auth_headers: dict[str, str], fake_llm: dict[str, Any]
) -> tuple[str, str]:
    """Create a group with one agent, send one message to lazily create the
    chat_thread, return (group_id, thread_id)."""
    fake_llm["messages"] = ["x"]
    workspace = await client.post(
        "/api/v1/workspaces",
        headers=auth_headers,
        json={
            "name": "Thread repo",
            "backend_type": "local",
            "local_path": str(Path.cwd()),
        },
    )
    assert workspace.status_code == 201, workspace.text
    workspace_id = cast(str, workspace.json()["id"])
    r = await client.post(
        "/api/v1/agents",
        headers=auth_headers,
        json={
            "name": "Echo",
            "system_prompt": "be brief",
            "workspace_id": workspace_id,
        },
    )
    assert r.status_code == 201, r.text
    agent_id = cast(str, r.json()["id"])
    r = await client.post(
        "/api/v1/groups",
        headers=auth_headers,
        json={"name": "G", "workspace_id": workspace_id, "initial_agents": [agent_id]},
    )
    assert r.status_code == 201, r.text
    group_id = cast(str, r.json()["id"])
    r = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo hi"},
    )
    assert r.status_code == 201, r.text
    thread_id = cast(str, r.json()["agent_replies"][0]["thread_id"])
    return group_id, thread_id


async def test_get_thread_as_member_returns_metadata(
    client: AsyncClient, auth_headers: dict[str, str], fake_llm: dict[str, Any]
) -> None:
    _, thread_id = await _setup_group_with_thread(client, auth_headers, fake_llm)
    r = await client.get(f"/api/v1/threads/{thread_id}", headers=auth_headers)
    assert r.status_code == 200
    body = r.json()
    assert body["thread_type"] == "chat_thread"
    assert body["status"] == "completed"


async def test_get_thread_missing_returns_404(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    r = await client.get(f"/api/v1/threads/{uuid4()}", headers=auth_headers)
    assert r.status_code == 404


async def test_get_thread_as_non_member_forbidden(
    client: AsyncClient, auth_headers: dict[str, str], fake_llm: dict[str, Any]
) -> None:
    _, thread_id = await _setup_group_with_thread(client, auth_headers, fake_llm)

    s = secrets.token_hex(4)
    other_email = f"other-{s}@example.com"
    await client.post(
        "/api/v1/auth/register",
        json={"email": other_email, "password": "valid-password-1", "name": "O"},
    )
    r = await client.post(
        "/api/v1/auth/login",
        json={"email": other_email, "password": "valid-password-1"},
    )
    other_headers = {"Authorization": f"Bearer {r.json()['access_token']}"}

    r = await client.get(f"/api/v1/threads/{thread_id}", headers=other_headers)
    assert r.status_code == 403


# ---------- resume ----------


async def test_resume_thread_when_not_paused_returns_409(
    client: AsyncClient, auth_headers: dict[str, str], fake_llm: dict[str, Any]
) -> None:
    _, thread_id = await _setup_group_with_thread(client, auth_headers, fake_llm)
    # The thread is `completed` after _setup, not paused.
    r = await client.post(f"/api/v1/threads/{thread_id}/resume", headers=auth_headers)
    assert r.status_code == 409
    assert "not paused" in r.text.lower()


async def test_resume_thread_missing_returns_404(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    r = await client.post(
        f"/api/v1/threads/{uuid4()}/resume", headers=auth_headers
    )
    assert r.status_code == 404


async def test_resume_thread_paused_streams_continuation(
    client: AsyncClient,
    auth_headers: dict[str, str],
    fake_llm: dict[str, Any],
    db_session: AsyncSession,
) -> None:
    """End-to-end: simulate Stop (flip statuses) → call resume → verify."""
    fake_llm["messages"] = ["partial reply"]
    _, thread_id_str = await _setup_group_with_thread(client, auth_headers, fake_llm)
    thread_id = UUID(thread_id_str)

    # Simulate a Stop click by manually flipping the most recent agent
    # message and the thread to interrupted/paused. The full
    # cancel-mid-stream path is exercised at runtime by the SSE endpoint;
    # this test focuses on the resume contract.
    msg = await db_session.scalar(
        select(Message)
        .where(Message.thread_id == thread_id, Message.sender_type == "agent")
        .order_by(Message.created_at.desc())
        .limit(1)
    )
    assert msg is not None
    msg.status = "interrupted"
    thread = await db_session.scalar(select(Thread).where(Thread.id == thread_id))
    assert thread is not None
    thread.status = "paused"
    await db_session.flush()

    fake_llm["messages"] = [" CONTINUED"]
    async with client.stream(
        "POST", f"/api/v1/threads/{thread_id}/resume", headers=auth_headers
    ) as resp:
        assert resp.status_code == 200
        events: dict[str, int] = {}
        current = ""
        async for line in resp.aiter_lines():
            if line.startswith("event:"):
                current = line.split(":", 1)[1].strip()
                events[current] = events.get(current, 0) + 1
        assert events.get("token", 0) > 0
        assert events.get("agent_message", 0) == 1
        assert events.get("done", 0) == 1

    await db_session.refresh(msg)
    await db_session.refresh(thread)
    assert msg.status == "visible"
    assert "CONTINUED" in (msg.content or "")
    assert thread.status == "completed"
