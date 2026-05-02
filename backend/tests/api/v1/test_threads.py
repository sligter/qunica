import secrets
from uuid import uuid4

from httpx import AsyncClient


async def _setup_group_with_thread(
    client: AsyncClient, auth_headers: dict[str, str], fake_llm: dict
) -> tuple[str, str]:
    """Create a group with one agent, send one message to lazily create the
    chat_thread, return (group_id, thread_id)."""
    fake_llm["messages"] = ["x"]
    r = await client.post(
        "/api/v1/agents",
        headers=auth_headers,
        json={"name": "Echo", "system_prompt": "be brief"},
    )
    agent_id = r.json()["id"]
    r = await client.post(
        "/api/v1/groups",
        headers=auth_headers,
        json={"name": "G", "initial_agents": [agent_id]},
    )
    group_id = r.json()["id"]
    r = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo hi"},
    )
    thread_id = r.json()["agent_replies"][0]["thread_id"]
    return group_id, thread_id


async def test_get_thread_as_member_returns_metadata(
    client: AsyncClient, auth_headers: dict[str, str], fake_llm: dict
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
    client: AsyncClient, auth_headers: dict[str, str], fake_llm: dict
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
