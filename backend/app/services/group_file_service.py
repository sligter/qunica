"""Group file service — upload, list, delete files attached to a group."""

from pathlib import Path
from uuid import UUID

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.exceptions import NotFoundError, PermissionDeniedError
from app.models.group_file import GroupFile
from app.models.group_member import GroupMember
from app.models.user import User


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


async def upload_file(
    db: AsyncSession,
    group_id: UUID,
    user: User,
    filename: str,
    file_bytes: bytes,
    mime_type: str | None,
) -> GroupFile:
    await _assert_member(db, group_id, user)

    storage_dir = Path("uploads") / "groups" / str(group_id) / "files"
    storage_dir.mkdir(parents=True, exist_ok=True)
    file_path = storage_dir / filename
    file_path.write_bytes(file_bytes)

    gf = GroupFile(
        group_id=group_id,
        uploader_id=user.id,
        filename=filename,
        file_path=str(file_path),
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
