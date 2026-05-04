from uuid import UUID

from sqlalchemy import Boolean, String, Text
from sqlalchemy.dialects.postgresql import JSONB
from sqlalchemy.dialects.postgresql import UUID as PgUUID
from sqlalchemy.orm import Mapped, mapped_column

from app.models.base import Base, TimestampMixin, UUIDPkMixin


class Group(Base, UUIDPkMixin, TimestampMixin):
    __tablename__ = "groups"

    owner_id: Mapped[UUID] = mapped_column(PgUUID(as_uuid=True), nullable=False)
    org_id: Mapped[UUID | None] = mapped_column(PgUUID(as_uuid=True), nullable=True)
    name: Mapped[str] = mapped_column(String(100), nullable=False)
    avatar_url: Mapped[str | None] = mapped_column(Text, nullable=True)
    description: Mapped[str | None] = mapped_column(Text, nullable=True)
    group_type: Mapped[str | None] = mapped_column(String(50), nullable=True)
    announcement: Mapped[str | None] = mapped_column(Text, nullable=True)
    memory_enabled: Mapped[bool] = mapped_column(Boolean, default=True, nullable=False)
    allow_agent_suggest_invite: Mapped[bool] = mapped_column(
        Boolean, default=True, nullable=False
    )
    allow_agent_create_task: Mapped[bool] = mapped_column(
        Boolean, default=True, nullable=False
    )
    default_agent_response_mode: Mapped[str] = mapped_column(
        String(50), default="mentioned_only", nullable=False
    )
    free_speech: Mapped[bool] = mapped_column(Boolean, default=False, nullable=False)
    allow_agent_free_mention: Mapped[bool] = mapped_column(
        Boolean, default=True, nullable=False
    )
    muted_agent_ids: Mapped[list[str] | None] = mapped_column(JSONB, nullable=True)
    admin_agent_ids: Mapped[list[str] | None] = mapped_column(JSONB, nullable=True)
    status: Mapped[str] = mapped_column(String(30), default="active", nullable=False)
