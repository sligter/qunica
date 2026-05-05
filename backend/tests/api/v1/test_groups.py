import os
import secrets
from pathlib import Path
from typing import cast

import pytest
from httpx import AsyncClient
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.models.group import Group
from app.models.group_member import GroupMember
from app.models.group_note import GroupNote
from app.models.user import User
from app.models.workspace import Workspace
from app.services import group_workspace_file_service


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


async def _configure_group_root(client: AsyncClient, headers: dict[str, str], root: Path) -> str:
    r = await client.patch(
        "/api/v1/settings/system",
        headers=headers,
        json={"group_workspace_root": str(root)},
    )
    assert r.status_code == 200, r.text
    return cast(str, r.json()["group_workspace_root"])


async def _create_workspace_at(client: AsyncClient, headers: dict[str, str], path: Path) -> str:
    r = await client.post(
        "/api/v1/workspaces",
        headers=headers,
        json={
            "name": "Test repo",
            "backend_type": "local",
            "local_path": str(path),
        },
    )
    assert r.status_code == 201, r.text
    return cast(str, r.json()["id"])


async def _create_workspace(client: AsyncClient, headers: dict[str, str]) -> str:
    return await _create_workspace_at(client, headers, Path.cwd())


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
    r = await client.get(f"/api/v1/workspaces/{workspace_id}", headers=auth_headers)
    assert r.status_code == 200, r.text
    assert r.json()["local_path"] == str(expected_dir.resolve())

    # Group agents endpoint should show Echo
    r = await client.get(f"/api/v1/groups/{body['id']}/agents", headers=auth_headers)
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
    r = await client.post("/api/v1/groups", headers=auth_headers, json={"name": "OwnedGroup"})
    assert r.status_code == 201
    owned_id = r.json()["id"]

    other_headers = await _new_user_headers(client)
    other_root = tmp_path / "other"
    other_root.mkdir()
    await _configure_group_root(client, other_headers, other_root)
    r = await client.post("/api/v1/groups", headers=other_headers, json={"name": "OtherGroup"})
    other_id = r.json()["id"]

    r = await client.get("/api/v1/groups", headers=auth_headers)
    ids = {g["id"] for g in r.json()}
    assert owned_id in ids
    assert other_id not in ids


async def test_get_group_as_non_member_forbidden(
    client: AsyncClient, auth_headers: dict[str, str], tmp_path: Path
) -> None:
    await _configure_group_root(client, auth_headers, tmp_path)
    r = await client.post("/api/v1/groups", headers=auth_headers, json={"name": "P"})
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


async def test_group_workspace_file_list_preview_rename_delete(
    client: AsyncClient, auth_headers: dict[str, str], tmp_path: Path
) -> None:
    workspace_root = tmp_path / "workspace"
    workspace_root.mkdir()
    (workspace_root / "docs").mkdir()
    (workspace_root / "docs" / "brief.txt").write_text("hello workspace", encoding="utf-8")
    (workspace_root / "image.bin").write_bytes(b"\x00\x01\x02")
    workspace_id = await _create_workspace_at(client, auth_headers, workspace_root)
    r = await client.post(
        "/api/v1/groups",
        headers=auth_headers,
        json={"name": "Workspace Files", "workspace_id": workspace_id},
    )
    assert r.status_code == 201, r.text
    group_id = r.json()["id"]

    r = await client.get(f"/api/v1/groups/{group_id}/workspace-files", headers=auth_headers)
    assert r.status_code == 200, r.text
    names = [item["name"] for item in r.json()]
    assert names == ["docs", "image.bin"]

    r = await client.get(
        f"/api/v1/groups/{group_id}/workspace-files/preview",
        headers=auth_headers,
        params={"path": "docs/brief.txt"},
    )
    assert r.status_code == 200, r.text
    assert r.json()["is_text"] is True
    assert r.json()["content"] == "hello workspace"

    r = await client.get(
        f"/api/v1/groups/{group_id}/workspace-files/preview",
        headers=auth_headers,
        params={"path": "image.bin"},
    )
    assert r.status_code == 200, r.text
    assert r.json()["is_text"] is False
    assert "not available" in r.json()["message"]

    r = await client.patch(
        f"/api/v1/groups/{group_id}/workspace-files/rename",
        headers=auth_headers,
        params={"path": "docs/brief.txt"},
        json={"new_path": "docs/renamed.txt"},
    )
    assert r.status_code == 200, r.text
    assert r.json()["path"] == "docs/renamed.txt"
    assert not (workspace_root / "docs" / "brief.txt").exists()
    assert (workspace_root / "docs" / "renamed.txt").exists()

    r = await client.delete(
        f"/api/v1/groups/{group_id}/workspace-files",
        headers=auth_headers,
        params={"path": "docs/renamed.txt"},
    )
    assert r.status_code == 204, r.text
    assert not (workspace_root / "docs" / "renamed.txt").exists()


async def test_group_workspace_file_upload_download_and_safety(
    client: AsyncClient,
    auth_headers: dict[str, str],
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    workspace_root = tmp_path / "workspace-upload"
    workspace_root.mkdir()
    workspace_id = await _create_workspace_at(client, auth_headers, workspace_root)
    r = await client.post(
        "/api/v1/groups",
        headers=auth_headers,
        json={"name": "Workspace Uploads", "workspace_id": workspace_id},
    )
    assert r.status_code == 201, r.text
    group_id = r.json()["id"]

    monkeypatch.setattr(group_workspace_file_service, "MAX_UPLOAD_BYTES", 4)
    r = await client.post(
        f"/api/v1/groups/{group_id}/workspace-files/upload",
        headers=auth_headers,
        files={"file": ("too-large.txt", b"12345", "text/plain")},
    )
    assert r.status_code == 400
    monkeypatch.setattr(group_workspace_file_service, "MAX_UPLOAD_BYTES", 25 * 1024 * 1024)

    r = await client.post(
        f"/api/v1/groups/{group_id}/workspace-files/upload",
        headers=auth_headers,
        files={"file": ("brief.txt", b"hello uploads", "text/plain")},
    )
    assert r.status_code == 201, r.text
    assert r.json()["path"] == "uploads/brief.txt"
    assert (workspace_root / "uploads" / "brief.txt").read_text(encoding="utf-8") == "hello uploads"

    r = await client.get(
        f"/api/v1/groups/{group_id}/workspace-files",
        headers=auth_headers,
        params={"path": "uploads"},
    )
    assert r.status_code == 200, r.text
    assert [item["name"] for item in r.json()] == ["brief.txt"]

    r = await client.get(
        f"/api/v1/groups/{group_id}/workspace-files/download",
        headers=auth_headers,
        params={"path": "uploads/brief.txt"},
    )
    assert r.status_code == 200, r.text
    assert r.content == b"hello uploads"
    assert "brief.txt" in r.headers["content-disposition"]

    r = await client.post(
        f"/api/v1/groups/{group_id}/workspace-files/upload",
        headers=auth_headers,
        files={"file": ("brief.txt", b"duplicate", "text/plain")},
    )
    assert r.status_code == 400
    assert (workspace_root / "uploads" / "brief.txt").read_text(encoding="utf-8") == "hello uploads"

    r = await client.post(
        f"/api/v1/groups/{group_id}/workspace-files/upload",
        headers=auth_headers,
        files={"file": ("", b"bad", "text/plain")},
    )
    assert r.status_code in {400, 422}

    unsafe_upload_names = [
        ".",
        "../escape.txt",
        "nested/escape.txt",
        r"nested\\escape.txt",
        "C:/escape.txt",
        "//server/share.txt",
    ]
    for unsafe_name in unsafe_upload_names:
        r = await client.post(
            f"/api/v1/groups/{group_id}/workspace-files/upload",
            headers=auth_headers,
            files={"file": (unsafe_name, b"bad", "text/plain")},
        )
        assert r.status_code == 400, unsafe_name

    r = await client.get(
        f"/api/v1/groups/{group_id}/workspace-files/download",
        headers=auth_headers,
        params={"path": "uploads"},
    )
    assert r.status_code == 400
    r = await client.get(
        f"/api/v1/groups/{group_id}/workspace-files/download",
        headers=auth_headers,
        params={"path": "../outside.txt"},
    )
    assert r.status_code == 400

    r = await client.post(
        f"/api/v1/groups/{group_id}/files",
        headers=auth_headers,
        files={"file": ("legacy.txt", b"legacy uploads", "text/plain")},
    )
    assert r.status_code == 201, r.text
    assert r.json()["filename"] == "legacy.txt"
    legacy_content = (workspace_root / "uploads" / "legacy.txt").read_text(encoding="utf-8")
    assert legacy_content == "legacy uploads"
    r = await client.post(
        f"/api/v1/groups/{group_id}/files",
        headers=auth_headers,
        files={"file": ("legacy.txt", b"duplicate", "text/plain")},
    )
    assert r.status_code == 400

    other_headers = await _new_user_headers(client)
    r = await client.post(
        f"/api/v1/groups/{group_id}/workspace-files/upload",
        headers=other_headers,
        files={"file": ("other.txt", b"bad", "text/plain")},
    )
    assert r.status_code == 403
    r = await client.get(
        f"/api/v1/groups/{group_id}/workspace-files/download",
        headers=other_headers,
        params={"path": "uploads/brief.txt"},
    )
    assert r.status_code == 403


async def test_group_workspace_file_paths_must_stay_inside_workspace(
    client: AsyncClient, auth_headers: dict[str, str], tmp_path: Path
) -> None:
    workspace_root = tmp_path / "workspace"
    workspace_root.mkdir()
    (workspace_root / "safe.txt").write_text("safe", encoding="utf-8")
    (workspace_root / "taken.txt").write_text("taken", encoding="utf-8")
    (workspace_root / "full-dir").mkdir()
    (workspace_root / "full-dir" / "nested.txt").write_text("nested", encoding="utf-8")
    unsafe_paths = ["../outside.txt", "/tmp/outside.txt", "C:/tmp/outside.txt"]
    outside = tmp_path / "outside.txt"
    outside.write_text("secret", encoding="utf-8")
    try:
        (workspace_root / "escape.txt").symlink_to(outside)
    except OSError:
        pass
    else:
        unsafe_paths.append("escape.txt")
    workspace_id = await _create_workspace_at(client, auth_headers, workspace_root)
    r = await client.post(
        "/api/v1/groups",
        headers=auth_headers,
        json={"name": "Workspace Safety", "workspace_id": workspace_id},
    )
    assert r.status_code == 201, r.text
    group_id = r.json()["id"]

    if os.name == "nt":
        unsafe_paths.append("/C:/tmp/outside.txt")
    for unsafe_path in unsafe_paths:
        r = await client.get(
            f"/api/v1/groups/{group_id}/workspace-files/preview",
            headers=auth_headers,
            params={"path": unsafe_path},
        )
        assert r.status_code == 400, unsafe_path

    r = await client.patch(
        f"/api/v1/groups/{group_id}/workspace-files/rename",
        headers=auth_headers,
        params={"path": "safe.txt"},
        json={"new_path": "../renamed.txt"},
    )
    assert r.status_code == 400

    r = await client.patch(
        f"/api/v1/groups/{group_id}/workspace-files/rename",
        headers=auth_headers,
        params={"path": "safe.txt"},
        json={"new_path": "taken.txt"},
    )
    assert r.status_code == 400

    r = await client.patch(
        f"/api/v1/groups/{group_id}/workspace-files/rename",
        headers=auth_headers,
        params={"path": ""},
        json={"new_path": "root-moved"},
    )
    assert r.status_code == 400

    r = await client.delete(
        f"/api/v1/groups/{group_id}/workspace-files",
        headers=auth_headers,
        params={"path": ""},
    )
    assert r.status_code == 400

    r = await client.delete(
        f"/api/v1/groups/{group_id}/workspace-files",
        headers=auth_headers,
        params={"path": "full-dir"},
    )
    assert r.status_code == 400
    assert (workspace_root / "full-dir").is_dir()


async def test_group_workspace_files_authorize_members_but_use_owner_workspace(
    client: AsyncClient,
    db_session: AsyncSession,
    auth_headers: dict[str, str],
    tmp_path: Path,
) -> None:
    me = await client.get("/api/v1/auth/me", headers=auth_headers)
    owner_id = me.json()["id"]
    owner = await db_session.scalar(select(User).where(User.id == owner_id))
    assert owner is not None
    workspace_root = tmp_path / "owner-workspace"
    workspace_root.mkdir()
    (workspace_root / "shared.txt").write_text("member can read", encoding="utf-8")
    workspace = Workspace(owner_id=owner.id, name="Owner workspace", local_path=str(workspace_root))
    db_session.add(workspace)
    await db_session.flush()
    group = Group(owner_id=owner.id, workspace_id=workspace.id, name="Shared workspace")
    db_session.add(group)
    await db_session.flush()
    member_headers = await _new_user_headers(client)
    member_me = await client.get("/api/v1/auth/me", headers=member_headers)
    member = await db_session.scalar(select(User).where(User.id == member_me.json()["id"]))
    assert member is not None
    db_session.add_all(
        [
            GroupMember(group_id=group.id, user_id=owner.id, role="owner"),
            GroupMember(group_id=group.id, user_id=member.id, role="member"),
        ]
    )
    await db_session.flush()

    r = await client.get(
        f"/api/v1/groups/{group.id}/workspace-files/preview",
        headers=member_headers,
        params={"path": "shared.txt"},
    )
    assert r.status_code == 200, r.text
    assert r.json()["content"] == "member can read"

    non_member_headers = await _new_user_headers(client)
    r = await client.get(
        f"/api/v1/groups/{group.id}/workspace-files",
        headers=non_member_headers,
    )
    assert r.status_code == 403


async def test_group_notes_are_stored_in_workspace_notes_directory(
    client: AsyncClient,
    db_session: AsyncSession,
    auth_headers: dict[str, str],
    tmp_path: Path,
) -> None:
    workspace_root = tmp_path / "notes-workspace"
    workspace_root.mkdir()
    workspace_id = await _create_workspace_at(client, auth_headers, workspace_root)
    r = await client.post(
        "/api/v1/groups",
        headers=auth_headers,
        json={"name": "Workspace Notes", "workspace_id": workspace_id},
    )
    assert r.status_code == 201, r.text
    group_id = r.json()["id"]

    r = await client.post(
        f"/api/v1/groups/{group_id}/notes",
        headers=auth_headers,
        json={"title": "Brief", "content": "first draft"},
    )
    assert r.status_code == 201, r.text
    note = r.json()
    note_path = workspace_root / "Notes" / f"{note['id']}.md"
    assert note_path.read_text(encoding="utf-8") == "first draft"
    db_note = await db_session.scalar(select(GroupNote).where(GroupNote.id == note["id"]))
    assert db_note is not None
    assert db_note.content == "first draft"

    r = await client.get(f"/api/v1/groups/{group_id}/notes", headers=auth_headers)
    assert r.status_code == 200, r.text
    assert r.json()[0]["content"] == "first draft"

    r = await client.patch(
        f"/api/v1/groups/{group_id}/notes/{note['id']}",
        headers=auth_headers,
        json={"content": "second draft"},
    )
    assert r.status_code == 200, r.text
    assert r.json()["content"] == "second draft"
    assert note_path.read_text(encoding="utf-8") == "second draft"
    await db_session.refresh(db_note)
    assert db_note.content == "second draft"

    r = await client.get(
        f"/api/v1/groups/{group_id}/workspace-files",
        headers=auth_headers,
        params={"path": "Notes"},
    )
    assert r.status_code == 200, r.text
    assert [item["name"] for item in r.json()] == [f"{note['id']}.md"]

    r = await client.delete(
        f"/api/v1/groups/{group_id}/notes/{note['id']}",
        headers=auth_headers,
    )
    assert r.status_code == 204, r.text
    assert not note_path.exists()
    r = await client.get(f"/api/v1/groups/{group_id}/notes", headers=auth_headers)
    assert r.status_code == 200, r.text
    assert r.json() == []


async def test_group_notes_authorize_and_reject_unsafe_notes_path(
    client: AsyncClient,
    db_session: AsyncSession,
    auth_headers: dict[str, str],
    tmp_path: Path,
) -> None:
    me = await client.get("/api/v1/auth/me", headers=auth_headers)
    owner = await db_session.scalar(select(User).where(User.id == me.json()["id"]))
    assert owner is not None
    workspace_root = tmp_path / "notes-safety"
    workspace_root.mkdir()
    outside = tmp_path / "outside-notes"
    outside.mkdir()
    notes_link = workspace_root / "Notes"
    try:
        notes_link.symlink_to(outside, target_is_directory=True)
    except OSError:
        pytest.skip("filesystem does not allow directory symlinks")
    workspace = Workspace(owner_id=owner.id, name="Unsafe notes", local_path=str(workspace_root))
    group = Group(owner_id=owner.id, workspace_id=workspace.id, name="Unsafe notes")
    db_session.add_all([workspace, group])
    await db_session.flush()
    db_session.add(GroupMember(group_id=group.id, user_id=owner.id, role="owner"))
    await db_session.flush()

    r = await client.post(
        f"/api/v1/groups/{group.id}/notes",
        headers=auth_headers,
        json={"title": "Bad", "content": "escape"},
    )
    assert r.status_code == 400
    assert not list(outside.iterdir())

    other_headers = await _new_user_headers(client)
    r = await client.get(f"/api/v1/groups/{group.id}/notes", headers=other_headers)
    assert r.status_code == 403


async def test_group_notes_reject_missing_and_non_local_workspace(
    client: AsyncClient,
    db_session: AsyncSession,
    auth_headers: dict[str, str],
) -> None:
    me = await client.get("/api/v1/auth/me", headers=auth_headers)
    owner = await db_session.scalar(select(User).where(User.id == me.json()["id"]))
    assert owner is not None
    no_workspace = Group(owner_id=owner.id, name="No notes workspace")
    cloud_workspace = Workspace(
        owner_id=owner.id,
        name="Cloud notes workspace",
        backend_type="cloud_sandbox",
        sandbox_ref="sandbox-notes",
    )
    db_session.add_all([no_workspace, cloud_workspace])
    await db_session.flush()
    cloud_group = Group(
        owner_id=owner.id, workspace_id=cloud_workspace.id, name="Cloud notes group"
    )
    db_session.add(cloud_group)
    await db_session.flush()
    db_session.add_all(
        [
            GroupMember(group_id=no_workspace.id, user_id=owner.id, role="owner"),
            GroupMember(group_id=cloud_group.id, user_id=owner.id, role="owner"),
        ]
    )
    await db_session.flush()

    r = await client.post(
        f"/api/v1/groups/{no_workspace.id}/notes",
        headers=auth_headers,
        json={"title": "Missing", "content": "body"},
    )
    assert r.status_code == 400
    r = await client.post(
        f"/api/v1/groups/{cloud_group.id}/notes",
        headers=auth_headers,
        json={"title": "Cloud", "content": "body"},
    )
    assert r.status_code == 400


async def test_group_workspace_files_reject_missing_and_non_local_workspace(
    client: AsyncClient,
    db_session: AsyncSession,
    auth_headers: dict[str, str],
) -> None:
    me = await client.get("/api/v1/auth/me", headers=auth_headers)
    owner_id = me.json()["id"]
    owner = await db_session.scalar(select(User).where(User.id == owner_id))
    assert owner is not None
    no_workspace = Group(owner_id=owner.id, name="No workspace")
    cloud_workspace = Workspace(
        owner_id=owner.id,
        name="Cloud workspace",
        backend_type="cloud_sandbox",
        sandbox_ref="sandbox-1",
    )
    db_session.add_all([no_workspace, cloud_workspace])
    await db_session.flush()
    cloud_group = Group(owner_id=owner.id, workspace_id=cloud_workspace.id, name="Cloud group")
    db_session.add(cloud_group)
    await db_session.flush()
    db_session.add_all(
        [
            GroupMember(group_id=no_workspace.id, user_id=owner.id, role="owner"),
            GroupMember(group_id=cloud_group.id, user_id=owner.id, role="owner"),
        ]
    )
    await db_session.flush()

    r = await client.get(
        f"/api/v1/groups/{no_workspace.id}/workspace-files",
        headers=auth_headers,
    )
    assert r.status_code == 400
    r = await client.get(
        f"/api/v1/groups/{cloud_group.id}/workspace-files",
        headers=auth_headers,
    )
    assert r.status_code == 400


async def test_add_agent_to_group_uses_display_name_fallback(
    client: AsyncClient, auth_headers: dict[str, str], tmp_path: Path
) -> None:
    await _configure_group_root(client, auth_headers, tmp_path)
    r = await client.post("/api/v1/groups", headers=auth_headers, json={"name": "G"})
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
