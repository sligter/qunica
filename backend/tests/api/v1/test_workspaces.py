from pathlib import Path

from httpx import AsyncClient


async def test_create_workspace_returns_201(
    client: AsyncClient,
    auth_headers: dict[str, str],
) -> None:
    r = await client.post(
        "/api/v1/workspaces",
        headers=auth_headers,
        json={
            "name": "Local repo",
            "backend_type": "local",
            "local_path": str(Path.cwd()),
        },
    )
    assert r.status_code == 201, r.text
    body = r.json()
    assert body["name"] == "Local repo"
    assert body["backend_type"] == "local"
    assert body["local_path"]


async def test_create_workspace_rejects_missing_local_path(
    client: AsyncClient,
    auth_headers: dict[str, str],
) -> None:
    r = await client.post(
        "/api/v1/workspaces",
        headers=auth_headers,
        json={"name": "Missing", "backend_type": "local"},
    )
    assert r.status_code == 400


async def test_create_workspace_rejects_nonexistent_directory(
    client: AsyncClient,
    auth_headers: dict[str, str],
) -> None:
    r = await client.post(
        "/api/v1/workspaces",
        headers=auth_headers,
        json={
            "name": "Gone",
            "backend_type": "local",
            "local_path": str(Path.cwd() / "does-not-exist-for-workspace-test"),
        },
    )
    assert r.status_code == 400


async def test_list_workspaces_only_returns_own(client: AsyncClient) -> None:
    import secrets

    suffix = secrets.token_hex(4)
    email1 = f"w1-{suffix}@example.com"
    await client.post(
        "/api/v1/auth/register",
        json={"email": email1, "password": "valid-password-1", "name": "W1"},
    )
    r = await client.post(
        "/api/v1/auth/login",
        json={"email": email1, "password": "valid-password-1"},
    )
    h1 = {"Authorization": f"Bearer {r.json()['access_token']}"}
    await client.post(
        "/api/v1/workspaces",
        headers=h1,
        json={
            "name": "OnlyForW1",
            "backend_type": "local",
            "local_path": str(Path.cwd()),
        },
    )

    email2 = f"w2-{suffix}@example.com"
    await client.post(
        "/api/v1/auth/register",
        json={"email": email2, "password": "valid-password-1", "name": "W2"},
    )
    r = await client.post(
        "/api/v1/auth/login",
        json={"email": email2, "password": "valid-password-1"},
    )
    h2 = {"Authorization": f"Bearer {r.json()['access_token']}"}

    r = await client.get("/api/v1/workspaces", headers=h1)
    assert r.status_code == 200
    assert any(workspace["name"] == "OnlyForW1" for workspace in r.json())

    r = await client.get("/api/v1/workspaces", headers=h2)
    assert r.status_code == 200
    assert all(workspace["name"] != "OnlyForW1" for workspace in r.json())
