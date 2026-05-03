"""Group note service — CRUD for shared notes within a group."""

from uuid import UUID

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.exceptions import NotFoundError, PermissionDeniedError
from app.models.group_member import GroupMember
from app.models.group_note import GroupNote
from app.models.user import User
from app.schemas.group_note import GroupNoteCreate, GroupNoteUpdate


async def _assert_member(db: AsyncSession, group_id: UUID, user: User) -> None:
    membership = await db.scalar(
        select(GroupMember).where(
            GroupMember.group_id == group_id,
            GroupMember.user_id == user.id,
            GroupMember.status == "active",
        )
    )
    if membership is None:
        raise PermissionDeniedError("not a member of this group")


async def create_note(
    db: AsyncSession, group_id: UUID, data: GroupNoteCreate, user: User
) -> GroupNote:
    await _assert_member(db, group_id, user)
    note = GroupNote(
        group_id=group_id,
        author_id=user.id,
        title=data.title,
        content=data.content,
    )
    db.add(note)
    await db.flush()
    await db.refresh(note)
    return note


async def list_notes(
    db: AsyncSession, group_id: UUID, user: User
) -> list[GroupNote]:
    await _assert_member(db, group_id, user)
    stmt = (
        select(GroupNote)
        .where(GroupNote.group_id == group_id, GroupNote.status == "active")
        .order_by(GroupNote.updated_at.desc())
    )
    return list(await db.scalars(stmt))


async def get_note(
    db: AsyncSession, group_id: UUID, note_id: UUID, user: User
) -> GroupNote:
    await _assert_member(db, group_id, user)
    note = await db.scalar(
        select(GroupNote).where(
            GroupNote.id == note_id,
            GroupNote.group_id == group_id,
            GroupNote.status == "active",
        )
    )
    if note is None:
        raise NotFoundError(f"group note {note_id}")
    return note


async def update_note(
    db: AsyncSession, group_id: UUID, note_id: UUID, data: GroupNoteUpdate, user: User
) -> GroupNote:
    note = await get_note(db, group_id, note_id, user)
    if data.title is not None:
        note.title = data.title
    if data.content is not None:
        note.content = data.content
    await db.flush()
    await db.refresh(note)
    return note


async def delete_note(
    db: AsyncSession, group_id: UUID, note_id: UUID, user: User
) -> None:
    note = await get_note(db, group_id, note_id, user)
    note.status = "deleted"
    await db.flush()
