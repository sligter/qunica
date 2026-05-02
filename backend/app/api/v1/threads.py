from uuid import UUID

from fastapi import APIRouter, Depends
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.deps import get_current_user
from app.core.exceptions import PermissionDeniedError
from app.db import get_db
from app.models.thread import Thread
from app.models.user import User
from app.schemas.thread import ThreadRead
from app.services import group_service, thread_service

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
