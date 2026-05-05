from uuid import UUID

from fastapi import APIRouter, Depends, Request, UploadFile, status
from fastapi.responses import FileResponse
from sqlalchemy.ext.asyncio import AsyncSession
from sse_starlette.sse import EventSourceResponse

from app.core.deps import get_current_user
from app.db import get_db
from app.models.agent import Agent
from app.models.group import Group
from app.models.group_agent import GroupAgent
from app.models.group_member import GroupMember
from app.models.message import Message
from app.models.user import User
from app.schemas.group import (
    ClearGroupMessagesResponse,
    GroupAgentAdd,
    GroupAgentMuteUpdate,
    GroupAgentRead,
    GroupAgentWorkspaceSharingUpdate,
    GroupCreate,
    GroupMemberAdd,
    GroupMemberMuteUpdate,
    GroupMemberRead,
    GroupRead,
    GroupUpdate,
)
from app.schemas.group_file import GroupFileRead
from app.schemas.group_note import GroupNoteCreate, GroupNoteRead, GroupNoteUpdate
from app.schemas.group_workspace_file import (
    GroupWorkspaceFilePreview,
    GroupWorkspaceFileRead,
    GroupWorkspaceFileRename,
)
from app.schemas.message import MessageCreate, MessageRead, MessageSendResponse
from app.schemas.user import UserRead
from app.services import (
    group_file_service,
    group_note_service,
    group_service,
    group_workspace_file_service,
    message_service,
)

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


# --- agents in group ---


def _to_group_agent_read(ga: GroupAgent, agent: Agent) -> GroupAgentRead:
    return GroupAgentRead(
        id=ga.id,
        group_id=ga.group_id,
        agent_id=ga.agent_id,
        display_name=ga.display_name or agent.name,
        role=ga.role,
        response_mode=ga.response_mode,
        share_group_workspace=group_service.is_group_workspace_shared(ga),
        status=ga.status,
        joined_at=ga.joined_at,
    )


def _to_group_member_read(gm: GroupMember, user: User, group: Group) -> GroupMemberRead:
    return GroupMemberRead(
        id=gm.id,
        group_id=gm.group_id,
        user_id=gm.user_id,
        display_name=user.name,
        role=gm.role,
        status=gm.status,
        is_muted=str(gm.user_id) in (group.muted_member_ids or []),
        joined_at=gm.joined_at,
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
    ga, agent = await group_service.add_agent(
        db,
        group_id,
        body.agent_id,
        current_user,
        share_group_workspace=body.share_group_workspace,
    )
    return _to_group_agent_read(ga, agent)


@router.get("/{group_id}/agents", response_model=list[GroupAgentRead])
async def list_group_agents(
    group_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> list[GroupAgentRead]:
    rows = await group_service.list_agents_in_group(db, group_id, current_user)
    return [_to_group_agent_read(ga, agent) for ga, agent in rows]


@router.patch(
    "/{group_id}/agents/{agent_id}/workspace-sharing",
    response_model=GroupAgentRead,
)
async def set_group_agent_workspace_sharing(
    group_id: UUID,
    agent_id: UUID,
    data: GroupAgentWorkspaceSharingUpdate,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> GroupAgentRead:
    ga, agent = await group_service.set_agent_workspace_sharing(
        db, group_id, agent_id, data.share_group_workspace, current_user
    )
    return _to_group_agent_read(ga, agent)


@router.delete("/{group_id}/agents/{agent_id}", status_code=status.HTTP_204_NO_CONTENT)
async def remove_agent_from_group(
    group_id: UUID,
    agent_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> None:
    await group_service.remove_agent(db, group_id, agent_id, current_user)


@router.patch("/{group_id}/agents/{agent_id}/mute", response_model=GroupAgentRead)
async def set_group_agent_muted(
    group_id: UUID,
    agent_id: UUID,
    data: GroupAgentMuteUpdate,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> GroupAgentRead:
    ga, agent, _group = await group_service.set_agent_muted(
        db, group_id, agent_id, data.muted, current_user
    )
    return _to_group_agent_read(ga, agent)


@router.get("/{group_id}/member-candidates", response_model=list[UserRead])
async def search_group_member_candidates(
    group_id: UUID,
    q: str = "",
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> list[User]:
    return await group_service.search_users_for_group(db, group_id, q, current_user)


# --- members in group ---


@router.get("/{group_id}/members", response_model=list[GroupMemberRead])
async def list_group_members(
    group_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> list[GroupMemberRead]:
    group = await group_service.get_group(db, group_id, current_user)
    rows = await group_service.list_members_in_group(db, group_id, current_user)
    return [_to_group_member_read(gm, user, group) for gm, user in rows]


@router.post(
    "/{group_id}/members",
    response_model=GroupMemberRead,
    status_code=status.HTTP_201_CREATED,
)
async def add_group_member(
    group_id: UUID,
    body: GroupMemberAdd,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> GroupMemberRead:
    gm, user = await group_service.add_member(db, group_id, body.user_id, current_user)
    group = await group_service.get_group(db, group_id, current_user)
    return _to_group_member_read(gm, user, group)


@router.delete("/{group_id}/members/{user_id}", status_code=status.HTTP_204_NO_CONTENT)
async def remove_group_member(
    group_id: UUID,
    user_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> None:
    await group_service.remove_member(db, group_id, user_id, current_user)


@router.patch("/{group_id}/members/{user_id}/mute", response_model=GroupMemberRead)
async def set_group_member_muted(
    group_id: UUID,
    user_id: UUID,
    data: GroupMemberMuteUpdate,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> GroupMemberRead:
    gm, user, group = await group_service.set_member_muted(
        db, group_id, user_id, data.muted, current_user
    )
    return _to_group_member_read(gm, user, group)


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
        silent_turns=result.silent_turns,
        all_silent=result.all_silent,
        waiting_for_user=result.waiting_for_user,
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


@router.post("/{group_id}/messages/clear", response_model=ClearGroupMessagesResponse)
async def clear_messages(
    group_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> ClearGroupMessagesResponse:
    cleared_count = await message_service.clear_group_history(db, group_id, current_user)
    return ClearGroupMessagesResponse(cleared_count=cleared_count)


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
        db,
        group_id,
        current_user,
        filename=file.filename or "",
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


# --- workspace files in group ---


@router.post(
    "/{group_id}/workspace-files/upload",
    response_model=GroupWorkspaceFileRead,
    status_code=status.HTTP_201_CREATED,
)
async def upload_group_workspace_file(
    group_id: UUID,
    file: UploadFile,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> GroupWorkspaceFileRead:
    content = await file.read()
    return await group_workspace_file_service.upload_workspace_file(
        db,
        group_id,
        current_user,
        filename=file.filename or "",
        file_bytes=content,
    )


@router.get("/{group_id}/workspace-files/download")
async def download_group_workspace_file(
    group_id: UUID,
    path: str,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> FileResponse:
    file_path, filename, media_type = await group_workspace_file_service.download_workspace_file(
        db, group_id, current_user, path
    )
    return FileResponse(path=file_path, filename=filename, media_type=media_type)


@router.get("/{group_id}/workspace-files", response_model=list[GroupWorkspaceFileRead])
async def list_group_workspace_files(
    group_id: UUID,
    path: str = "",
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> list[GroupWorkspaceFileRead]:
    return await group_workspace_file_service.list_workspace_files(
        db, group_id, current_user, path
    )


@router.get("/{group_id}/workspace-files/preview", response_model=GroupWorkspaceFilePreview)
async def preview_group_workspace_file(
    group_id: UUID,
    path: str,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> GroupWorkspaceFilePreview:
    return await group_workspace_file_service.preview_workspace_file(
        db, group_id, current_user, path
    )


@router.patch("/{group_id}/workspace-files/rename", response_model=GroupWorkspaceFileRead)
async def rename_group_workspace_file(
    group_id: UUID,
    path: str,
    data: GroupWorkspaceFileRename,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> GroupWorkspaceFileRead:
    return await group_workspace_file_service.rename_workspace_file(
        db, group_id, current_user, path, data.new_path
    )


@router.delete("/{group_id}/workspace-files", status_code=status.HTTP_204_NO_CONTENT)
async def delete_group_workspace_file(
    group_id: UUID,
    path: str,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> None:
    await group_workspace_file_service.delete_workspace_file(db, group_id, current_user, path)


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
