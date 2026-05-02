from uuid import UUID

from fastapi import APIRouter, Depends, Request, status
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
)
from app.schemas.message import MessageCreate, MessageRead, MessageSendResponse
from app.services import group_service, message_service

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
