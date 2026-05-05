"""Group note service — CRUD for shared notes within a group workspace."""

import os
from datetime import UTC, datetime
from pathlib import Path
from uuid import UUID

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm.attributes import set_committed_value

from app.core.exceptions import AgentChatError, NotFoundError, PermissionDeniedError
from app.models.group import Group
from app.models.group_member import GroupMember
from app.models.group_note import GroupNote
from app.models.user import User
from app.models.workspace import Workspace
from app.schemas.group_note import GroupNoteCreate, GroupNoteUpdate

NOTES_DIR = "Notes"
NOTE_FILE_SUFFIX = ".md"


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
        raise AgentChatError("group notes require a local workspace")
    if workspace.local_path is None:
        raise AgentChatError("local workspace has no local_path")
    root = Path(workspace.local_path).resolve()
    if not root.exists() or not root.is_dir():
        raise AgentChatError("group workspace path is not an existing directory")
    return root


def _notes_dir(root: Path, *, create: bool = False) -> Path:
    notes_dir = root / NOTES_DIR
    if create:
        notes_dir.mkdir(parents=True, exist_ok=True)
    resolved = notes_dir.resolve(strict=True)
    if os.path.commonpath([str(root), str(resolved)]) != str(root):
        raise AgentChatError("group notes path escapes the group workspace")
    if not resolved.is_dir():
        raise AgentChatError("group notes path is not a directory")
    return resolved


def _note_path(root: Path, note_id: UUID) -> Path:
    notes_dir = _notes_dir(root, create=True)
    path = (notes_dir / f"{note_id}{NOTE_FILE_SUFFIX}").resolve(strict=False)
    if os.path.commonpath([str(root), str(path)]) != str(root):
        raise AgentChatError("group note path escapes the group workspace")
    return path


def _read_note_content(root: Path, note_id: UUID) -> str:
    path = _note_path(root, note_id)
    if not path.exists():
        return ""
    if not path.is_file():
        raise AgentChatError("group note path is not a file")
    return path.read_text(encoding="utf-8")


def _write_note_content(root: Path, note_id: UUID, content: str) -> None:
    path = _note_path(root, note_id)
    path.write_text(content, encoding="utf-8")


def _delete_note_content(root: Path, note_id: UUID) -> None:
    path = _note_path(root, note_id)
    if path.exists():
        if not path.is_file():
            raise AgentChatError("group note path is not a file")
        path.unlink()


async def create_note(
    db: AsyncSession, group_id: UUID, data: GroupNoteCreate, user: User
) -> GroupNote:
    root = await _workspace_root(db, group_id, user)
    note = GroupNote(
        group_id=group_id,
        author_id=user.id,
        title=data.title,
        content=data.content,
    )
    db.add(note)
    await db.flush()
    _write_note_content(root, note.id, data.content)
    await db.refresh(note)
    set_committed_value(note, "content", data.content)
    return note


async def list_notes(
    db: AsyncSession, group_id: UUID, user: User
) -> list[GroupNote]:
    root = await _workspace_root(db, group_id, user)
    stmt = (
        select(GroupNote)
        .where(GroupNote.group_id == group_id, GroupNote.status == "active")
        .order_by(GroupNote.updated_at.desc())
    )
    notes = list(await db.scalars(stmt))
    for note in notes:
        set_committed_value(note, "content", _read_note_content(root, note.id))
    return notes


async def get_note(
    db: AsyncSession, group_id: UUID, note_id: UUID, user: User
) -> GroupNote:
    root = await _workspace_root(db, group_id, user)
    note = await db.scalar(
        select(GroupNote).where(
            GroupNote.id == note_id,
            GroupNote.group_id == group_id,
            GroupNote.status == "active",
        )
    )
    if note is None:
        raise NotFoundError(f"group note {note_id}")
    set_committed_value(note, "content", _read_note_content(root, note.id))
    return note


async def update_note(
    db: AsyncSession, group_id: UUID, note_id: UUID, data: GroupNoteUpdate, user: User
) -> GroupNote:
    root = await _workspace_root(db, group_id, user)
    note = await get_note(db, group_id, note_id, user)
    if data.title is not None:
        note.title = data.title
    if data.content is not None:
        note.content = data.content
        _write_note_content(root, note.id, data.content)
    note.updated_at = datetime.now(UTC)
    await db.flush()
    await db.refresh(note)
    set_committed_value(note, "content", _read_note_content(root, note.id))
    return note


async def delete_note(
    db: AsyncSession, group_id: UUID, note_id: UUID, user: User
) -> None:
    root = await _workspace_root(db, group_id, user)
    note = await get_note(db, group_id, note_id, user)
    _delete_note_content(root, note.id)
    note.status = "deleted"
    set_committed_value(note, "content", "")
    await db.flush()
