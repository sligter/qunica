"""Thread service: per-(group, agent) chat_thread lifecycle.

Phase 1 Week 3-4 only uses `chat_thread`. One thread row per (group_id,
agent_id, thread_type='chat_thread'), reused across invocations.
"""

from datetime import UTC, datetime
from uuid import UUID

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.exceptions import NotFoundError
from app.models.thread import Thread


async def get_or_create_chat_thread(
    db: AsyncSession,
    group_id: UUID,
    agent_id: UUID,
    created_by: UUID,
) -> Thread:
    existing = await db.scalar(
        select(Thread).where(
            Thread.group_id == group_id,
            Thread.agent_id == agent_id,
            Thread.thread_type == "chat_thread",
        )
    )
    if existing is not None:
        return existing

    thread = Thread(
        group_id=group_id,
        agent_id=agent_id,
        created_by=created_by,
        thread_type="chat_thread",
        status="created",
        started_at=datetime.now(UTC),
    )
    db.add(thread)
    await db.flush()
    await db.refresh(thread)
    return thread


async def get_thread(db: AsyncSession, thread_id: UUID) -> Thread:
    thread = await db.scalar(select(Thread).where(Thread.id == thread_id))
    if thread is None:
        raise NotFoundError(f"thread {thread_id}")
    return thread


async def mark_running(db: AsyncSession, thread: Thread) -> None:
    thread.status = "running"
    if thread.started_at is None:
        thread.started_at = datetime.now(UTC)
    await db.flush()


async def mark_completed(db: AsyncSession, thread: Thread) -> None:
    thread.status = "completed"
    thread.completed_at = datetime.now(UTC)
    await db.flush()


async def mark_failed(db: AsyncSession, thread: Thread) -> None:
    thread.status = "failed"
    thread.completed_at = datetime.now(UTC)
    await db.flush()


async def mark_paused(db: AsyncSession, thread: Thread) -> None:
    """Set thread.status='paused'. Used when an in-flight agent reply is
    interrupted by the user. Does NOT set completed_at — the thread is not
    done; resume can flip it back to running.
    """
    thread.status = "paused"
    await db.flush()
