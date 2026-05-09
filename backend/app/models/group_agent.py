from datetime import datetime
from typing import Any
from uuid import UUID

from sqlalchemy import CheckConstraint, DateTime, Integer, String, UniqueConstraint, func
from sqlalchemy.dialects.postgresql import JSONB
from sqlalchemy.dialects.postgresql import UUID as PgUUID
from sqlalchemy.orm import Mapped, mapped_column

from app.models.base import Base, UUIDPkMixin


class GroupAgent(Base, UUIDPkMixin):
    __tablename__ = "group_agents"
    __table_args__ = (
        UniqueConstraint("group_id", "agent_id", name="uq_group_agents_group_agent"),
        CheckConstraint(
            "topology_role IS NULL OR topology_role IN ('hub', 'leader', 'worker')",
            name="ck_group_agents_topology_role",
        ),
        CheckConstraint(
            "speaking_order IS NULL OR speaking_order >= 1",
            name="ck_group_agents_speaking_order_min",
        ),
    )

    group_id: Mapped[UUID] = mapped_column(PgUUID(as_uuid=True), nullable=False)
    agent_id: Mapped[UUID] = mapped_column(PgUUID(as_uuid=True), nullable=False)
    display_name: Mapped[str | None] = mapped_column(String(100), nullable=True)
    role: Mapped[str | None] = mapped_column(String(50), nullable=True)
    topology_role: Mapped[str | None] = mapped_column(String(30), nullable=True)
    speaking_order: Mapped[int | None] = mapped_column(Integer, nullable=True)
    response_mode: Mapped[str] = mapped_column(
        String(50), default="mentioned_only", nullable=False
    )
    permissions: Mapped[dict[str, Any] | None] = mapped_column(JSONB, nullable=True)
    context_scope: Mapped[dict[str, Any] | None] = mapped_column(JSONB, nullable=True)
    file_scope: Mapped[dict[str, Any] | None] = mapped_column(JSONB, nullable=True)
    approval_policy: Mapped[dict[str, Any] | None] = mapped_column(JSONB, nullable=True)
    status: Mapped[str] = mapped_column(String(30), default="active", nullable=False)
    joined_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), server_default=func.now(), nullable=False
    )
    updated_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True),
        server_default=func.now(),
        onupdate=func.now(),
        nullable=False,
    )
