from httpx import AsyncClient


async def _create_agent(
    client: AsyncClient, headers: dict[str, str], name: str = "Echo"
) -> dict:
    r = await client.post(
        "/api/v1/agents",
        headers=headers,
        json={
            "name": name,
            "description": f"{name} description",
            "system_prompt": f"You are {name}. End with DONE.",
        },
    )
    assert r.status_code == 201, r.text
    return r.json()


async def test_create_agent_returns_201(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    a = await _create_agent(client, auth_headers, name="Nova")
    assert a["name"] == "Nova"
    assert a["visibility"] == "private"
    assert a["status"] == "active"


async def test_list_agents_only_returns_own(client: AsyncClient) -> None:
    # User 1 creates an agent
    import secrets

    s1 = secrets.token_hex(4)
    email1 = f"u1-{s1}@example.com"
    await client.post(
        "/api/v1/auth/register",
        json={"email": email1, "password": "valid-password-1", "name": "U1"},
    )
    r = await client.post(
        "/api/v1/auth/login",
        json={"email": email1, "password": "valid-password-1"},
    )
    h1 = {"Authorization": f"Bearer {r.json()['access_token']}"}
    await _create_agent(client, h1, name="OnlyForU1")

    # User 2 has none
    s2 = secrets.token_hex(4)
    email2 = f"u2-{s2}@example.com"
    await client.post(
        "/api/v1/auth/register",
        json={"email": email2, "password": "valid-password-1", "name": "U2"},
    )
    r = await client.post(
        "/api/v1/auth/login",
        json={"email": email2, "password": "valid-password-1"},
    )
    h2 = {"Authorization": f"Bearer {r.json()['access_token']}"}

    r = await client.get("/api/v1/agents", headers=h1)
    assert r.status_code == 200
    assert any(a["name"] == "OnlyForU1" for a in r.json())

    r = await client.get("/api/v1/agents", headers=h2)
    assert r.status_code == 200
    assert all(a["name"] != "OnlyForU1" for a in r.json())


async def test_get_other_users_agent_forbidden(client: AsyncClient) -> None:
    import secrets

    s1 = secrets.token_hex(4)
    email1 = f"o1-{s1}@example.com"
    await client.post(
        "/api/v1/auth/register",
        json={"email": email1, "password": "valid-password-1", "name": "O1"},
    )
    r = await client.post(
        "/api/v1/auth/login",
        json={"email": email1, "password": "valid-password-1"},
    )
    h1 = {"Authorization": f"Bearer {r.json()['access_token']}"}
    a = await _create_agent(client, h1, name="Private")

    s2 = secrets.token_hex(4)
    email2 = f"o2-{s2}@example.com"
    await client.post(
        "/api/v1/auth/register",
        json={"email": email2, "password": "valid-password-1", "name": "O2"},
    )
    r = await client.post(
        "/api/v1/auth/login",
        json={"email": email2, "password": "valid-password-1"},
    )
    h2 = {"Authorization": f"Bearer {r.json()['access_token']}"}

    r = await client.get(f"/api/v1/agents/{a['id']}", headers=h2)
    assert r.status_code == 403


async def test_invoke_uses_fake_llm(
    client: AsyncClient,
    auth_headers: dict[str, str],
    fake_llm: dict,
) -> None:
    fake_llm["messages"] = ["hello from fake"]
    a = await _create_agent(client, auth_headers, name="Fakeable")
    r = await client.post(
        f"/api/v1/agents/{a['id']}/invoke",
        headers=auth_headers,
        json={"message": "anything"},
    )
    assert r.status_code == 200
    assert r.json()["content"] == "hello from fake"


async def test_invoke_stream_emits_token_and_done(
    client: AsyncClient,
    auth_headers: dict[str, str],
    fake_llm: dict,
) -> None:
    fake_llm["messages"] = ["streamed reply"]
    a = await _create_agent(client, auth_headers, name="Streamable")
    events = []
    async with client.stream(
        "POST",
        f"/api/v1/agents/{a['id']}/invoke/stream",
        headers=auth_headers,
        json={"message": "tell me"},
    ) as resp:
        assert resp.status_code == 200
        async for line in resp.aiter_lines():
            if line.startswith("event:"):
                events.append(line.split(":", 1)[1].strip())
    assert "token" in events
    assert "done" in events
