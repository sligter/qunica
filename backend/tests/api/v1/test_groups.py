import secrets

from httpx import AsyncClient


async def _new_user_headers(client: AsyncClient) -> dict[str, str]:
    s = secrets.token_hex(4)
    email = f"g-{s}@example.com"
    await client.post(
        "/api/v1/auth/register",
        json={"email": email, "password": "valid-password-1", "name": "G"},
    )
    r = await client.post(
        "/api/v1/auth/login",
        json={"email": email, "password": "valid-password-1"},
    )
    return {"Authorization": f"Bearer {r.json()['access_token']}"}


async def _create_agent(client: AsyncClient, headers: dict[str, str], name: str) -> str:
    r = await client.post(
        "/api/v1/agents",
        headers=headers,
        json={"name": name, "system_prompt": f"You are {name}."},
    )
    return r.json()["id"]


async def test_create_group_with_initial_agents(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    agent_id = await _create_agent(client, auth_headers, "Echo")
    r = await client.post(
        "/api/v1/groups",
        headers=auth_headers,
        json={
            "name": "Project A",
            "description": "Test group",
            "announcement": "Be brief.",
            "initial_agents": [agent_id],
        },
    )
    assert r.status_code == 201
    body = r.json()
    assert body["name"] == "Project A"
    group_id = body["id"]

    # Group agents endpoint should show Echo
    r = await client.get(f"/api/v1/groups/{group_id}/agents", headers=auth_headers)
    assert r.status_code == 200
    assert len(r.json()) == 1
    assert r.json()[0]["display_name"] == "Echo"


async def test_list_groups_only_returns_owned(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    r = await client.post(
        "/api/v1/groups",
        headers=auth_headers,
        json={"name": "OwnedGroup"},
    )
    assert r.status_code == 201
    owned_id = r.json()["id"]

    # Another user
    other_headers = await _new_user_headers(client)
    r = await client.post(
        "/api/v1/groups", headers=other_headers, json={"name": "OtherGroup"}
    )
    other_id = r.json()["id"]

    r = await client.get("/api/v1/groups", headers=auth_headers)
    ids = {g["id"] for g in r.json()}
    assert owned_id in ids
    assert other_id not in ids


async def test_get_group_as_non_member_forbidden(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    r = await client.post("/api/v1/groups", headers=auth_headers, json={"name": "P"})
    group_id = r.json()["id"]

    other_headers = await _new_user_headers(client)
    r = await client.get(f"/api/v1/groups/{group_id}", headers=other_headers)
    assert r.status_code == 403


async def test_add_agent_to_group_uses_display_name_fallback(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    r = await client.post("/api/v1/groups", headers=auth_headers, json={"name": "G"})
    group_id = r.json()["id"]
    agent_id = await _create_agent(client, auth_headers, "Helper")
    r = await client.post(
        f"/api/v1/groups/{group_id}/agents",
        headers=auth_headers,
        json={"agent_id": agent_id},
    )
    assert r.status_code == 201
    assert r.json()["display_name"] == "Helper"
