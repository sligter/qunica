"""Workspace-rooted file browsing and management for group chat."""

import mimetypes
import os
import re
from datetime import UTC, datetime
from pathlib import Path, PurePosixPath
from uuid import UUID

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.exceptions import AgentChatError, NotFoundError, PermissionDeniedError
from app.models.group import Group
from app.models.group_member import GroupMember
from app.models.user import User
from app.models.workspace import Workspace
from app.schemas.group_workspace_file import GroupWorkspaceFilePreview, GroupWorkspaceFileRead

MAX_PREVIEW_BYTES = 64 * 1024
TEXT_PREVIEW_CHARS = 20_000
_TEXT_EXTENSIONS = {
    ".txt",
    ".md",
    ".markdown",
    ".csv",
    ".json",
    ".jsonl",
    ".yaml",
    ".yml",
    ".toml",
    ".ini",
    ".cfg",
    ".log",
    ".xml",
    ".html",
    ".css",
    ".js",
    ".jsx",
    ".ts",
    ".tsx",
    ".py",
    ".sh",
    ".bat",
    ".ps1",
    ".sql",
    ".rst",
}
_DRIVE_PREFIX_RE = re.compile(r"^[A-Za-z]:")


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


async def _workspace_root(db: AsyncSession, group_id: UUID, user: User) -> Path:
    group = await _assert_member(db, group_id, user)
    if group.workspace_id is None:
        raise AgentChatError("group has no bound workspace")
    workspace = await db.scalar(
        select(Workspace).where(Workspace.id == group.workspace_id, Workspace.status == "active")
    )
    if workspace is None:
        raise NotFoundError(f"workspace {group.workspace_id}")
    if workspace.owner_id != group.owner_id:
        raise PermissionDeniedError("group workspace is not accessible")
    if workspace.backend_type != "local":
        raise AgentChatError("group workspace files require a local workspace")
    if workspace.local_path is None:
        raise AgentChatError("local workspace has no local_path")
    root = Path(workspace.local_path).resolve()
    if not root.exists() or not root.is_dir():
        raise AgentChatError("group workspace path is not an existing directory")
    return root


def _validate_relative_path(path: str) -> PurePosixPath:
    normalized = path.strip().replace("\\", "/")
    if normalized in {"", "."}:
        return PurePosixPath(".")
    candidate = PurePosixPath(normalized)
    if (
        candidate.is_absolute()
        or normalized.startswith("//")
        or _DRIVE_PREFIX_RE.match(normalized) is not None
        or any(part in {"", ".", ".."} for part in candidate.parts)
    ):
        raise AgentChatError(
            "workspace file paths must be relative and stay inside the group workspace"
        )
    return candidate


def _resolve_inside(root: Path, path: str, *, must_exist: bool = True) -> Path:
    relative = _validate_relative_path(path)
    target = root if str(relative) == "." else root.joinpath(*relative.parts)
    resolved = target.resolve(strict=must_exist)
    if os.path.commonpath([str(root), str(resolved)]) != str(root):
        raise AgentChatError("workspace file path escapes the group workspace")
    return resolved


def _display_path(root: Path, path: Path) -> str:
    rel = path.relative_to(root)
    value = rel.as_posix()
    return value if value != "." else ""


def _file_read(path: Path, root: Path) -> GroupWorkspaceFileRead:
    stat = path.stat()
    return GroupWorkspaceFileRead(
        path=_display_path(root, path),
        name=path.name,
        is_dir=path.is_dir(),
        size=None if path.is_dir() else stat.st_size,
        modified_at=datetime.fromtimestamp(stat.st_mtime, UTC),
    )


def _looks_text(path: Path, sample: bytes) -> bool:
    if b"\x00" in sample:
        return False
    mime_type, _encoding = mimetypes.guess_type(path.name)
    if mime_type is not None and mime_type.startswith("text/"):
        return True
    if path.suffix.lower() in _TEXT_EXTENSIONS:
        return True
    try:
        sample.decode("utf-8")
    except UnicodeDecodeError:
        return False
    return True


async def list_workspace_files(
    db: AsyncSession, group_id: UUID, user: User, path: str = ""
) -> list[GroupWorkspaceFileRead]:
    root = await _workspace_root(db, group_id, user)
    directory = _resolve_inside(root, path or ".")
    if not directory.is_dir():
        raise AgentChatError("workspace path is not a directory")
    rows = [_file_read(item, root) for item in directory.iterdir() if not item.name.startswith(".")]
    rows.sort(key=lambda item: (not item.is_dir, item.name.lower()))
    return rows


async def preview_workspace_file(
    db: AsyncSession, group_id: UUID, user: User, path: str
) -> GroupWorkspaceFilePreview:
    root = await _workspace_root(db, group_id, user)
    file_path = _resolve_inside(root, path)
    if not file_path.is_file():
        raise AgentChatError("workspace path is not a file")
    size = file_path.stat().st_size
    with file_path.open("rb") as handle:
        sample = handle.read(MAX_PREVIEW_BYTES + 1)
    if not _looks_text(file_path, sample[: min(len(sample), 4096)]):
        return GroupWorkspaceFilePreview(
            path=_display_path(root, file_path),
            name=file_path.name,
            is_text=False,
            message="Preview is not available for binary or unsupported files.",
            size=size,
        )
    truncated = len(sample) > MAX_PREVIEW_BYTES
    content = sample[:MAX_PREVIEW_BYTES].decode("utf-8", errors="replace")
    if len(content) > TEXT_PREVIEW_CHARS:
        content = content[:TEXT_PREVIEW_CHARS]
        truncated = True
    return GroupWorkspaceFilePreview(
        path=_display_path(root, file_path),
        name=file_path.name,
        is_text=True,
        content=content,
        truncated=truncated,
        size=size,
    )


async def delete_workspace_file(db: AsyncSession, group_id: UUID, user: User, path: str) -> None:
    root = await _workspace_root(db, group_id, user)
    target = _resolve_inside(root, path)
    if target == root:
        raise AgentChatError("cannot delete the workspace root")
    if target.is_dir():
        try:
            target.rmdir()
        except OSError as exc:
            raise AgentChatError("directory must be empty before it can be deleted") from exc
    elif target.is_file():
        target.unlink()
    else:
        raise AgentChatError("workspace path is not a file or directory")


async def rename_workspace_file(
    db: AsyncSession, group_id: UUID, user: User, path: str, new_path: str
) -> GroupWorkspaceFileRead:
    root = await _workspace_root(db, group_id, user)
    source = _resolve_inside(root, path)
    destination = _resolve_inside(root, new_path, must_exist=False)
    if source == root:
        raise AgentChatError("cannot rename the workspace root")
    if destination.exists():
        raise AgentChatError("destination already exists")
    parent = destination.parent.resolve(strict=True)
    if os.path.commonpath([str(root), str(parent)]) != str(root):
        raise AgentChatError("workspace file path escapes the group workspace")
    source.rename(destination)
    return _file_read(destination.resolve(strict=True), root)
