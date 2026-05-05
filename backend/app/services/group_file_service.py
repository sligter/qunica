"""Group file service — upload, list, delete files attached to a group."""

import os
from pathlib import Path, PurePath
from uuid import UUID

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.exceptions import AgentChatError, NotFoundError, PermissionDeniedError
from app.models.group import Group
from app.models.group_file import GroupFile
from app.models.group_member import GroupMember
from app.models.user import User
from app.services import workspace_service


async def _assert_member(db: AsyncSession, group_id: UUID, user: User) -> Group:
    group = await db.scalar(select(Group).where(Group.id == group_id, Group.status == "active"))
    if group is None:
        raise NotFoundError(f"group {group_id}")
    membership = await db.scalar(
        select(GroupMember).where(
            GroupMember.group_id == group_id,
            GroupMember.user_id == user.id,
            GroupMember.status == "active",
        )
    )
    if membership is None:
        raise PermissionDeniedError("not a member of this group")
    return group


def _safe_display_filename(filename: str) -> str:
    candidate = filename.strip()
    if not candidate:
        raise AgentChatError("upload filename is required")
    path = PurePath(candidate)
    normalized = candidate.replace("\\", "/")
    if (
        path.is_absolute()
        or path.name != candidate
        or normalized.startswith("//")
        or ":" in candidate
        or any(part in {"", ".", ".."} for part in normalized.split("/"))
    ):
        raise AgentChatError("upload filename must be a plain filename")
    return candidate


async def upload_file(
    db: AsyncSession,
    group_id: UUID,
    user: User,
    filename: str,
    file_bytes: bytes,
    mime_type: str | None,
) -> GroupFile:
    group = await _assert_member(db, group_id, user)
    if group.workspace_id is None:
        raise AgentChatError("group has no bound workspace")
    workspace = await workspace_service.get_active_workspace(db, group.workspace_id, user)
    if workspace.backend_type == "cloud_sandbox":
        raise AgentChatError("group file storage is not supported for cloud sandbox workspaces")
    if workspace.local_path is None:
        raise AgentChatError("local workspace has no local_path")

    display_filename = _safe_display_filename(filename)
    root = Path(workspace.local_path).resolve()
    storage_dir = root / "uploads"
    storage_dir.mkdir(parents=True, exist_ok=True)
    resolved_storage_dir = storage_dir.resolve(strict=True)
    if os.path.commonpath([str(root), str(resolved_storage_dir)]) != str(root):
        raise AgentChatError("group upload path escapes the group workspace")
    file_path = storage_dir / display_filename
    if file_path.exists():
        raise AgentChatError("a file with this name already exists in uploads")
    file_path.write_bytes(file_bytes)

    gf = GroupFile(
        group_id=group_id,
        uploader_id=user.id,
        filename=display_filename,
        file_path=str(file_path.resolve()),
        file_size=len(file_bytes),
        mime_type=mime_type,
    )
    db.add(gf)
    await db.flush()
    await db.refresh(gf)
    return gf


async def list_files(
    db: AsyncSession, group_id: UUID, user: User
) -> list[GroupFile]:
    await _assert_member(db, group_id, user)
    stmt = (
        select(GroupFile)
        .where(GroupFile.group_id == group_id, GroupFile.status == "active")
        .order_by(GroupFile.created_at.desc())
    )
    return list(await db.scalars(stmt))


async def delete_file(
    db: AsyncSession, group_id: UUID, file_id: UUID, user: User
) -> None:
    await _assert_member(db, group_id, user)
    gf = await db.scalar(
        select(GroupFile).where(
            GroupFile.id == file_id,
            GroupFile.group_id == group_id,
        )
    )
    if gf is None:
        raise NotFoundError(f"group file {file_id}")
    gf.status = "deleted"
    await db.flush()
