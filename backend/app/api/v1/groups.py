from uuid import UUID

from fastapi import APIRouter, Depends, Request, UploadFile, status
from sqlalchemy.ext.asyncio import AsyncSession
from sse_starlette.sse import EventSourceResponse

from app.core.deps import get_current_user
from app.db import get_db
from app.models.group import Group
from app.models.message import Message
from app.models.user import User
from app.schemas.group import (
    GroupAgentAdd,
    GroupAgentRead,
    GroupCreate,
    GroupRead,
    GroupUpdate,
)
from app.schemas.group_file import GroupFileRead
from app.schemas.group_note import GroupNoteCreate, GroupNoteRead, GroupNoteUpdate
from app.schemas.message import MessageCreate, MessageRead, MessageSendResponse
from app.services import group_file_service, group_note_service, group_service, message_service

router = APIRouter(prefix="/groups", tags=["groups"])


# --- group resource ---


@router.post("", response_model=GroupRead, status_code=status.HTTP_201_CREATED)
async def create_group(
    data: GroupCreate,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> Group:
    return await group_service.create_group(db, data, current_user)


@router.get("", response_model=list[GroupRead])
async def list_groups(
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> list[Group]:
    return await group_service.list_groups_for_user(db, current_user)


@router.get("/{group_id}", response_model=GroupRead)
async def get_group(
    group_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> Group:
    return await group_service.get_group(db, group_id, current_user)


@router.patch("/{group_id}", response_model=GroupRead)
async def update_group(
    group_id: UUID,
    data: GroupUpdate,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> Group:
    return await group_service.update_group(db, group_id, data, current_user)


@router.delete("/{group_id}", status_code=status.HTTP_204_NO_CONTENT)
async def delete_group(
    group_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> None:
    await group_service.delete_group(db, group_id, current_user)


# --- agents in group ---


def _to_group_agent_read(ga, agent) -> GroupAgentRead:  # type: ignore[no-untyped-def]
    return GroupAgentRead(
        id=ga.id,
        group_id=ga.group_id,
        agent_id=ga.agent_id,
        display_name=ga.display_name or agent.name,
        role=ga.role,
        response_mode=ga.response_mode,
        status=ga.status,
        joined_at=ga.joined_at,
    )


@router.post(
    "/{group_id}/agents",
    response_model=GroupAgentRead,
    status_code=status.HTTP_201_CREATED,
)
async def add_agent_to_group(
    group_id: UUID,
    body: GroupAgentAdd,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> GroupAgentRead:
    ga, agent = await group_service.add_agent(db, group_id, body.agent_id, current_user)
    return _to_group_agent_read(ga, agent)


@router.get("/{group_id}/agents", response_model=list[GroupAgentRead])
async def list_group_agents(
    group_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> list[GroupAgentRead]:
    rows = await group_service.list_agents_in_group(db, group_id, current_user)
    return [_to_group_agent_read(ga, agent) for ga, agent in rows]


# --- messages in group ---


@router.post(
    "/{group_id}/messages",
    response_model=MessageSendResponse,
    status_code=status.HTTP_201_CREATED,
)
async def send_message(
    group_id: UUID,
    body: MessageCreate,
    request: Request,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> MessageSendResponse:
    result = await message_service.send_message(
        db, request, group_id, current_user, body.content
    )
    return MessageSendResponse(
        user_message=MessageRead.model_validate(result.user_message),
        agent_replies=[MessageRead.model_validate(m) for m in result.agent_replies],
        warnings=result.warnings,
    )


@router.post("/{group_id}/messages/stream")
async def send_message_stream(
    group_id: UUID,
    body: MessageCreate,
    request: Request,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> EventSourceResponse:
    return EventSourceResponse(
        message_service.send_message_stream(
            db, request, group_id, current_user, body.content
        )
    )


@router.get("/{group_id}/messages", response_model=list[MessageRead])
async def list_messages(
    group_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
    limit: int = 50,
) -> list[Message]:
    return await message_service.list_messages(db, group_id, current_user, limit=limit)


# --- files in group ---


@router.post(
    "/{group_id}/files",
    response_model=GroupFileRead,
    status_code=status.HTTP_201_CREATED,
)
async def upload_group_file(
    group_id: UUID,
    file: UploadFile,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> GroupFileRead:
    content = await file.read()
    gf = await group_file_service.upload_file(
        db, group_id, current_user,
        filename=file.filename or "untitled",
        file_bytes=content,
        mime_type=file.content_type,
    )
    return GroupFileRead.model_validate(gf)


@router.get("/{group_id}/files", response_model=list[GroupFileRead])
async def list_group_files(
    group_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> list[GroupFileRead]:
    rows = await group_file_service.list_files(db, group_id, current_user)
    return [GroupFileRead.model_validate(f) for f in rows]


@router.delete("/{group_id}/files/{file_id}", status_code=status.HTTP_204_NO_CONTENT)
async def delete_group_file(
    group_id: UUID,
    file_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> None:
    await group_file_service.delete_file(db, group_id, file_id, current_user)


# --- notes in group ---


@router.post(
    "/{group_id}/notes",
    response_model=GroupNoteRead,
    status_code=status.HTTP_201_CREATED,
)
async def create_group_note(
    group_id: UUID,
    data: GroupNoteCreate,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> GroupNoteRead:
    note = await group_note_service.create_note(db, group_id, data, current_user)
    return GroupNoteRead.model_validate(note)


@router.get("/{group_id}/notes", response_model=list[GroupNoteRead])
async def list_group_notes(
    group_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> list[GroupNoteRead]:
    rows = await group_note_service.list_notes(db, group_id, current_user)
    return [GroupNoteRead.model_validate(n) for n in rows]


@router.patch("/{group_id}/notes/{note_id}", response_model=GroupNoteRead)
async def update_group_note(
    group_id: UUID,
    note_id: UUID,
    data: GroupNoteUpdate,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> GroupNoteRead:
    note = await group_note_service.update_note(db, group_id, note_id, data, current_user)
    return GroupNoteRead.model_validate(note)


@router.delete("/{group_id}/notes/{note_id}", status_code=status.HTTP_204_NO_CONTENT)
async def delete_group_note(
    group_id: UUID,
    note_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> None:
    await group_note_service.delete_note(db, group_id, note_id, current_user)
