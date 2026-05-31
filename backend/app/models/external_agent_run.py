from datetime import datetime
from uuid import UUID

from sqlalchemy import DateTime, Integer, String, Text, func
from sqlalchemy.orm import Mapped, mapped_column

from app.models.base import Base, UUIDPkMixin
from app.models.types import GUID, JSONData


class ExternalAgentRun(Base, UUIDPkMixin):
    __tablename__ = "external_agent_runs"

    owner_id: Mapped[UUID] = mapped_column(GUID(), nullable=False, index=True)
    group_id: Mapped[UUID | None] = mapped_column(GUID(), nullable=True, index=True)
    agent_id: Mapped[UUID] = mapped_column(GUID(), nullable=False, index=True)
    thread_id: Mapped[UUID | None] = mapped_column(GUID(), nullable=True, index=True)
    adapter: Mapped[str] = mapped_column(String(50), nullable=False)
    cwd: Mapped[str] = mapped_column(Text, nullable=False)
    status: Mapped[str] = mapped_column(String(30), default="running", nullable=False)
    argv: Mapped[list[str]] = mapped_column(JSONData, default=list, nullable=False)
    exit_code: Mapped[int | None] = mapped_column(Integer, nullable=True)
    stdout_tail: Mapped[str | None] = mapped_column(Text, nullable=True)
    stderr_tail: Mapped[str | None] = mapped_column(Text, nullable=True)
    error_message: Mapped[str | None] = mapped_column(Text, nullable=True)
    started_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), server_default=func.now(), nullable=False
    )
    ended_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True), nullable=True)
