import secrets
from pathlib import Path
from typing import cast

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


async def _configure_group_root(
    client: AsyncClient, headers: dict[str, str], root: Path
) -> str:
    r = await client.patch(
        "/api/v1/settings/system",
        headers=headers,
        json={"group_workspace_root": str(root)},
    )
    assert r.status_code == 200, r.text
    return cast(str, r.json()["group_workspace_root"])


async def _create_workspace(client: AsyncClient, headers: dict[str, str]) -> str:
    r = await client.post(
        "/api/v1/workspaces",
        headers=headers,
        json={
            "name": "Test repo",
            "backend_type": "local",
            "local_path": str(Path.cwd()),
        },
    )
    assert r.status_code == 201, r.text
    return cast(str, r.json()["id"])


async def _create_agent(client: AsyncClient, headers: dict[str, str], name: str) -> str:
    workspace_id = await _create_workspace(client, headers)
    r = await client.post(
        "/api/v1/agents",
        headers=headers,
        json={
            "name": name,
            "system_prompt": f"You are {name}.",
            "workspace_id": workspace_id,
        },
    )
    assert r.status_code == 201, r.text
    return cast(str, r.json()["id"])


async def test_create_group_auto_creates_dedicated_workspace(
    client: AsyncClient, auth_headers: dict[str, str], tmp_path: Path
) -> None:
    await _configure_group_root(client, auth_headers, tmp_path)
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
    assert r.status_code == 201, r.text
    body = r.json()
    assert body["name"] == "Project A"
    workspace_id = body["workspace_id"]
    assert workspace_id is not None
    expected_dir = tmp_path / body["id"]
    assert expected_dir.is_dir()

    # The dedicated workspace is owned by the same user and points at the
    # auto-created directory.
    r = await client.get(
        f"/api/v1/workspaces/{workspace_id}", headers=auth_headers
    )
    assert r.status_code == 200, r.text
    assert r.json()["local_path"] == str(expected_dir.resolve())

    # Group agents endpoint should show Echo
    r = await client.get(
        f"/api/v1/groups/{body['id']}/agents", headers=auth_headers
    )
    assert r.status_code == 200
    assert len(r.json()) == 1
    assert r.json()[0]["display_name"] == "Echo"


async def test_create_group_without_root_returns_400(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    r = await client.post("/api/v1/groups", headers=auth_headers, json={"name": "X"})
    assert r.status_code == 400, r.text
    assert "group workspace root" in r.json()["error"]["message"].lower()


async def test_create_group_explicit_workspace_still_supported(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    # Even when system root is unset, an explicit owned workspace works.
    workspace_id = await _create_workspace(client, auth_headers)
    r = await client.post(
        "/api/v1/groups",
        headers=auth_headers,
        json={"workspace_id": workspace_id, "name": "Explicit"},
    )
    assert r.status_code == 201, r.text
    assert r.json()["workspace_id"] == workspace_id


async def test_list_groups_only_returns_owned(
    client: AsyncClient, auth_headers: dict[str, str], tmp_path: Path
) -> None:
    await _configure_group_root(client, auth_headers, tmp_path)
    r = await client.post(
        "/api/v1/groups", headers=auth_headers, json={"name": "OwnedGroup"}
    )
    assert r.status_code == 201
    owned_id = r.json()["id"]

    other_headers = await _new_user_headers(client)
    other_root = tmp_path / "other"
    other_root.mkdir()
    await _configure_group_root(client, other_headers, other_root)
    r = await client.post(
        "/api/v1/groups", headers=other_headers, json={"name": "OtherGroup"}
    )
    other_id = r.json()["id"]

    r = await client.get("/api/v1/groups", headers=auth_headers)
    ids = {g["id"] for g in r.json()}
    assert owned_id in ids
    assert other_id not in ids


async def test_get_group_as_non_member_forbidden(
    client: AsyncClient, auth_headers: dict[str, str], tmp_path: Path
) -> None:
    await _configure_group_root(client, auth_headers, tmp_path)
    r = await client.post(
        "/api/v1/groups", headers=auth_headers, json={"name": "P"}
    )
    group_id = r.json()["id"]

    other_headers = await _new_user_headers(client)
    r = await client.get(f"/api/v1/groups/{group_id}", headers=other_headers)
    assert r.status_code == 403


async def test_create_group_rejects_other_owner_workspace(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    other_headers = await _new_user_headers(client)
    other_workspace_id = await _create_workspace(client, other_headers)
    r = await client.post(
        "/api/v1/groups",
        headers=auth_headers,
        json={"name": "Bad workspace", "workspace_id": other_workspace_id},
    )
    assert r.status_code == 403


async def test_add_agent_to_group_uses_display_name_fallback(
    client: AsyncClient, auth_headers: dict[str, str], tmp_path: Path
) -> None:
    await _configure_group_root(client, auth_headers, tmp_path)
    r = await client.post(
        "/api/v1/groups", headers=auth_headers, json={"name": "G"}
    )
    group_id = r.json()["id"]
    agent_id = await _create_agent(client, auth_headers, "Helper")
    r = await client.post(
        f"/api/v1/groups/{group_id}/agents",
        headers=auth_headers,
        json={"agent_id": agent_id, "share_group_workspace": True},
    )
    assert r.status_code == 201
    assert r.json()["display_name"] == "Helper"
    assert r.json()["share_group_workspace"] is True

    r = await client.patch(
        f"/api/v1/groups/{group_id}/agents/{agent_id}/workspace-sharing",
        headers=auth_headers,
        json={"share_group_workspace": False},
    )
    assert r.status_code == 200
    assert r.json()["share_group_workspace"] is False
