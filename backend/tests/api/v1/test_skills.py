import io
import zipfile
from pathlib import Path

from httpx import AsyncClient


def _zip_bytes(entries: dict[str, bytes]) -> bytes:
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as zf:
        for path, payload in entries.items():
            zf.writestr(path, payload)
    return buffer.getvalue()


async def _import_package(client: AsyncClient, headers: dict[str, str]) -> dict[str, object]:
    payload = _zip_bytes(
        {
            "bundle/SKILL.md": b"---\nname: Editable\ndescription: Demo\n---\nBody\n",
            "bundle/references/guide.md": b"# Guide\n",
            "bundle/assets/icon.png": b"\x89PNG\r\n\x1a\n",
        }
    )
    response = await client.post(
        "/api/v1/skills/import-package",
        headers=headers,
        files={"file": ("skill.zip", payload, "application/zip")},
    )
    assert response.status_code == 201, response.text
    return dict(response.json())


async def test_skill_resource_list_read_and_update(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    skill = await _import_package(client, auth_headers)

    response = await client.get(
        f"/api/v1/skills/{skill['id']}/resources", headers=auth_headers
    )
    assert response.status_code == 200, response.text
    resources = response.json()
    guide = next(item for item in resources if item["path"] == "references/guide.md")
    assert guide["is_text"] is True
    icon = next(item for item in resources if item["path"] == "assets/icon.png")
    assert icon["is_text"] is False

    response = await client.get(
        f"/api/v1/skills/{skill['id']}/resources/references/guide.md",
        headers=auth_headers,
    )
    assert response.status_code == 200, response.text
    assert response.json()["content"] == "# Guide\n"

    response = await client.patch(
        f"/api/v1/skills/{skill['id']}/resources/references/guide.md",
        headers=auth_headers,
        json={"content": "# Updated\n"},
    )
    assert response.status_code == 200, response.text
    assert response.json()["content"] == "# Updated\n"

    response = await client.get(
        f"/api/v1/skills/{skill['id']}/resources/references/guide.md",
        headers=auth_headers,
    )
    assert response.status_code == 200, response.text
    assert response.json()["content"] == "# Updated\n"


async def test_skill_resource_rejects_traversal(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    skill = await _import_package(client, auth_headers)

    response = await client.get(
        f"/api/v1/skills/{skill['id']}/resources/../SKILL.md",
        headers=auth_headers,
    )
    assert response.status_code in {400, 404}


async def test_skill_resource_rejects_binary_edit(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    skill = await _import_package(client, auth_headers)

    response = await client.patch(
        f"/api/v1/skills/{skill['id']}/resources/assets/icon.png",
        headers=auth_headers,
        json={"content": "not a png"},
    )
    assert response.status_code == 400


async def test_skill_resource_owner_scoped(client: AsyncClient) -> None:
    import secrets

    s1 = secrets.token_hex(4)
    email1 = f"skill-owner-{s1}@example.com"
    await client.post(
        "/api/v1/auth/register",
        json={"email": email1, "password": "valid-password-1", "name": "Owner"},
    )
    response = await client.post(
        "/api/v1/auth/login",
        json={"email": email1, "password": "valid-password-1"},
    )
    h1 = {"Authorization": f"Bearer {response.json()['access_token']}"}
    skill = await _import_package(client, h1)

    s2 = secrets.token_hex(4)
    email2 = f"skill-other-{s2}@example.com"
    await client.post(
        "/api/v1/auth/register",
        json={"email": email2, "password": "valid-password-1", "name": "Other"},
    )
    response = await client.post(
        "/api/v1/auth/login",
        json={"email": email2, "password": "valid-password-1"},
    )
    h2 = {"Authorization": f"Bearer {response.json()['access_token']}"}

    response = await client.get(
        f"/api/v1/skills/{skill['id']}/resources", headers=h2
    )
    assert response.status_code == 403


async def test_delete_agent_hides_from_list_and_get(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    workspace_response = await client.post(
        "/api/v1/workspaces",
        headers=auth_headers,
        json={
            "name": "Repo",
            "backend_type": "local",
            "local_path": str(Path.cwd()),
        },
    )
    assert workspace_response.status_code == 201, workspace_response.text
    agent_response = await client.post(
        "/api/v1/agents",
        headers=auth_headers,
        json={
            "name": "Delete Me",
            "system_prompt": "You can be deleted.",
            "workspace_id": workspace_response.json()["id"],
        },
    )
    assert agent_response.status_code == 201, agent_response.text
    agent_id = agent_response.json()["id"]

    response = await client.delete(f"/api/v1/agents/{agent_id}", headers=auth_headers)
    assert response.status_code == 204, response.text

    response = await client.get("/api/v1/agents", headers=auth_headers)
    assert response.status_code == 200, response.text
    assert all(agent["id"] != agent_id for agent in response.json())

    response = await client.get(f"/api/v1/agents/{agent_id}", headers=auth_headers)
    assert response.status_code == 404
