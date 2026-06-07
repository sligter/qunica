import io
import zipfile
from pathlib import Path
from typing import Any
from uuid import UUID

from httpx import AsyncClient
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.config import PROJECT_ROOT
from app.models.skill import Skill
from app.services import skill_service


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
            "bundle/references/chart-impl.md": "# Chart implementation\n\nUTF-8: café\n".encode(),
            "bundle/references/not-utf8.md": b"\xff\xfe\x00\x00",
            "bundle/scripts/render.py": b"print('chart')\n",
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
    assert guide["content"] is None
    chart = next(item for item in resources if item["path"] == "references/chart-impl.md")
    assert chart["is_text"] is True
    script = next(item for item in resources if item["path"] == "scripts/render.py")
    assert script["is_text"] is True
    not_utf8 = next(item for item in resources if item["path"] == "references/not-utf8.md")
    assert not_utf8["is_text"] is False
    icon = next(item for item in resources if item["path"] == "assets/icon.png")
    assert icon["is_text"] is False

    response = await client.get(
        f"/api/v1/skills/{skill['id']}/resources/references/guide.md",
        headers=auth_headers,
    )
    assert response.status_code == 200, response.text
    assert response.json()["content"] == "# Guide\n"

    response = await client.get(
        f"/api/v1/skills/{skill['id']}/resources/references/chart-impl.md",
        headers=auth_headers,
    )
    assert response.status_code == 200, response.text
    assert response.json()["is_text"] is True
    assert response.json()["content"] == "# Chart implementation\n\nUTF-8: café\n"

    response = await client.get(
        f"/api/v1/skills/{skill['id']}/resources/scripts/render.py",
        headers=auth_headers,
    )
    assert response.status_code == 200, response.text
    assert response.json()["content"] == "print('chart')\n"

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


async def test_import_skill_from_github_downloads_and_installs_package(
    client: AsyncClient,
    auth_headers: dict[str, str],
    monkeypatch: Any,
) -> None:
    archive = _zip_bytes(
        {
            "repo-main/docs/readme.md": b"# Not a skill\n",
            "repo-main/skills/demo/SKILL.md": (
                b"---\nname: GitHub Demo\ndescription: From GitHub\n---\nBody\n"
            ),
            "repo-main/skills/demo/references/guide.md": b"# Guide\n",
        }
    )
    calls: list[str] = []

    class FakeResponse:
        status_code = 200
        headers = {"content-length": str(len(archive))}
        content = archive

        def raise_for_status(self) -> None:
            return None

    class FakeAsyncClient:
        def __init__(self, *args: Any, **kwargs: Any) -> None:
            _ = args, kwargs

        async def __aenter__(self) -> "FakeAsyncClient":
            return self

        async def __aexit__(self, *args: Any) -> None:
            _ = args

        async def get(self, url: str) -> FakeResponse:
            calls.append(url)
            return FakeResponse()

    monkeypatch.setattr(skill_service.httpx, "AsyncClient", FakeAsyncClient)

    response = await client.post(
        "/api/v1/skills/import-github",
        headers=auth_headers,
        json={
            "url": "https://github.com/example/repo/tree/main/skills/demo",
        },
    )

    assert response.status_code == 201, response.text
    body = response.json()
    assert body["name"] == "GitHub Demo"
    assert body["source"] == "github"
    assert body["metadata"]["github"] == {
        "owner": "example",
        "repo": "repo",
        "branch": "main",
        "path": "skills/demo",
    }
    assert [item["path"] for item in body["files"]] == [
        "SKILL.md",
        "references/guide.md",
    ]
    assert calls == [
        "https://codeload.github.com/example/repo/zip/refs/heads/main"
    ]


async def test_import_skill_from_github_rejects_non_github_url(
    client: AsyncClient,
    auth_headers: dict[str, str],
) -> None:
    response = await client.post(
        "/api/v1/skills/import-github",
        headers=auth_headers,
        json={"url": "https://example.com/owner/repo"},
    )

    assert response.status_code == 400


async def test_skill_resource_survives_backend_cwd_changes(
    client: AsyncClient,
    auth_headers: dict[str, str],
    db_session: AsyncSession,
) -> None:
    skill = await _import_package(client, auth_headers)

    db_skill = await db_session.scalar(
        select(Skill).where(Skill.id == UUID(str(skill["id"])))
    )
    assert db_skill is not None
    assert db_skill.storage_path is not None
    relative_storage = Path(db_skill.storage_path).relative_to(PROJECT_ROOT)
    db_skill.storage_path = str(relative_storage)
    await db_session.flush()

    response = await client.get(
        f"/api/v1/skills/{skill['id']}/resources",
        headers=auth_headers,
    )
    assert response.status_code == 200, response.text
    resources = response.json()
    chart = next(item for item in resources if item["path"] == "references/chart-impl.md")
    assert chart["is_text"] is True

    response = await client.get(
        f"/api/v1/skills/{skill['id']}/resources/references/chart-impl.md",
        headers=auth_headers,
    )
    assert response.status_code == 200, response.text
    assert response.json()["content"] == "# Chart implementation\n\nUTF-8: café\n"


async def test_skill_resource_rejects_traversal(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    skill = await _import_package(client, auth_headers)

    response = await client.get(
        f"/api/v1/skills/{skill['id']}/resources/../SKILL.md",
        headers=auth_headers,
    )
    assert response.status_code in {400, 404}


async def test_skill_resource_rejects_binary_and_non_utf8_edit(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    skill = await _import_package(client, auth_headers)

    response = await client.get(
        f"/api/v1/skills/{skill['id']}/resources/references/not-utf8.md",
        headers=auth_headers,
    )
    assert response.status_code == 200, response.text
    assert response.json()["is_text"] is False
    assert response.json()["content"] is None

    response = await client.patch(
        f"/api/v1/skills/{skill['id']}/resources/references/not-utf8.md",
        headers=auth_headers,
        json={"content": "not utf-8"},
    )
    assert response.status_code == 400

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
