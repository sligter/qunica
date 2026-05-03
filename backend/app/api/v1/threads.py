from uuid import UUID

from fastapi import APIRouter, Depends, Request
from sqlalchemy.ext.asyncio import AsyncSession
from sse_starlette.sse import EventSourceResponse

from app.core.deps import get_current_user
from app.core.exceptions import PermissionDeniedError
from app.db import get_db
from app.models.thread import Thread
from app.models.user import User
from app.schemas.thread import ThreadRead
from app.services import group_service, message_service, thread_service

router = APIRouter(prefix="/threads", tags=["threads"])


@router.get("/{thread_id}", response_model=ThreadRead)
async def get_thread(
    thread_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> Thread:
    thread = await thread_service.get_thread(db, thread_id)
    # Membership check via the thread's group.
    try:
        await group_service.get_group(db, thread.group_id, current_user)
    except PermissionDeniedError as exc:
        raise PermissionDeniedError("not a member of this thread's group") from exc
    return thread


@router.post("/{thread_id}/resume")
async def resume_thread(
    thread_id: UUID,
    request: Request,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> EventSourceResponse:
    """Resume a paused thread; SSE-streams the agent's continuation.

    Pre-flight validation runs synchronously so 404 / 403 / 409 surface as
    proper HTTP errors before the SSE response starts. Streams the same
    `token` / `agent_message` / `done` events as the normal
    `POST /groups/{id}/messages/stream` endpoint, except no `user_message`
    event is emitted (this isn't a new user turn).
    """
    thread, agent, group, interrupted_msg = await message_service.resolve_resume_target(
        db, thread_id, current_user
    )
    return EventSourceResponse(
        message_service.resume_thread_stream(
            db, request, thread, agent, group, interrupted_msg
        )
    )
