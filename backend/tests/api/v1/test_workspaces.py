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


async def test_update_workspace_renames(
    client: AsyncClient,
    auth_headers: dict[str, str],
) -> None:
    created = await client.post(
        "/api/v1/workspaces",
        headers=auth_headers,
        json={
            "name": "Original",
            "backend_type": "local",
            "local_path": str(Path.cwd()),
        },
    )
    assert created.status_code == 201, created.text
    workspace_id = created.json()["id"]

    renamed = await client.patch(
        f"/api/v1/workspaces/{workspace_id}",
        headers=auth_headers,
        json={"name": "Renamed workspace"},
    )
    assert renamed.status_code == 200, renamed.text
    assert renamed.json()["name"] == "Renamed workspace"

    fetched = await client.get(f"/api/v1/workspaces/{workspace_id}", headers=auth_headers)
    assert fetched.status_code == 200, fetched.text
    assert fetched.json()["name"] == "Renamed workspace"


async def test_delete_workspace_soft_deletes_and_clears_active_bindings(
    client: AsyncClient,
    auth_headers: dict[str, str],
) -> None:
    created = await client.post(
        "/api/v1/workspaces",
        headers=auth_headers,
        json={
            "name": "Delete me",
            "backend_type": "local",
            "local_path": str(Path.cwd()),
        },
    )
    assert created.status_code == 201, created.text
    workspace_id = created.json()["id"]

    agent_response = await client.post(
        "/api/v1/agents",
        headers=auth_headers,
        json={
            "name": "Workspace Agent",
            "system_prompt": "Use the workspace.",
            "workspace_id": workspace_id,
        },
    )
    assert agent_response.status_code == 201, agent_response.text
    agent_id = agent_response.json()["id"]

    group_response = await client.post(
        "/api/v1/groups",
        headers=auth_headers,
        json={"name": "Workspace Group", "workspace_id": workspace_id},
    )
    assert group_response.status_code == 201, group_response.text
    group_id = group_response.json()["id"]

    deleted = await client.delete(
        f"/api/v1/workspaces/{workspace_id}",
        headers=auth_headers,
    )
    assert deleted.status_code == 204, deleted.text

    fetched_workspace = await client.get(
        f"/api/v1/workspaces/{workspace_id}",
        headers=auth_headers,
    )
    assert fetched_workspace.status_code == 200, fetched_workspace.text
    assert fetched_workspace.json()["status"] == "deleted"

    listed = await client.get("/api/v1/workspaces", headers=auth_headers)
    assert listed.status_code == 200, listed.text
    assert all(workspace["id"] != workspace_id for workspace in listed.json())

    fetched_agent = await client.get(f"/api/v1/agents/{agent_id}", headers=auth_headers)
    assert fetched_agent.status_code == 200, fetched_agent.text
    assert fetched_agent.json()["workspace_id"] is None

    fetched_group = await client.get(f"/api/v1/groups/{group_id}", headers=auth_headers)
    assert fetched_group.status_code == 200, fetched_group.text
    assert fetched_group.json()["workspace_id"] is None
