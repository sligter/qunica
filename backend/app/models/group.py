from uuid import UUID

from sqlalchemy import Boolean, CheckConstraint, Integer, String, Text, false, text
from sqlalchemy.dialects.postgresql import JSONB
from sqlalchemy.dialects.postgresql import UUID as PgUUID
from sqlalchemy.orm import Mapped, mapped_column

from app.models.base import Base, TimestampMixin, UUIDPkMixin


class Group(Base, UUIDPkMixin, TimestampMixin):
    __tablename__ = "groups"
    __table_args__ = (
        CheckConstraint(
            "proactive_max_rounds BETWEEN 1 AND 5",
            name="ck_groups_proactive_max_rounds_range",
        ),
    )

    owner_id: Mapped[UUID] = mapped_column(PgUUID(as_uuid=True), nullable=False)
    workspace_id: Mapped[UUID | None] = mapped_column(
        PgUUID(as_uuid=True), nullable=True, index=True
    )
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
    proactive_mode: Mapped[bool] = mapped_column(
        Boolean,
        default=False,
        server_default=false(),
        nullable=False,
    )
    proactive_max_rounds: Mapped[int] = mapped_column(
        Integer,
        default=1,
        server_default=text("1"),
        nullable=False,
    )
    allow_agent_free_mention: Mapped[bool] = mapped_column(
        Boolean, default=True, nullable=False
    )
    muted_agent_ids: Mapped[list[str] | None] = mapped_column(JSONB, nullable=True)
    admin_agent_ids: Mapped[list[str] | None] = mapped_column(JSONB, nullable=True)
    muted_member_ids: Mapped[list[str] | None] = mapped_column(JSONB, nullable=True)
    status: Mapped[str] = mapped_column(String(30), default="active", nullable=False)
