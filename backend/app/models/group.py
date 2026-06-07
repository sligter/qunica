from datetime import datetime
from uuid import UUID

from sqlalchemy import Boolean, CheckConstraint, DateTime, Integer, String, Text, false, text
from sqlalchemy.orm import Mapped, mapped_column

from app.models.base import Base, TimestampMixin, UUIDPkMixin
from app.models.types import GUID, JSONData


class Group(Base, UUIDPkMixin, TimestampMixin):
    __tablename__ = "groups"
    __table_args__ = (
        CheckConstraint(
            "proactive_max_rounds BETWEEN 1 AND 5",
            name="ck_groups_proactive_max_rounds_range",
        ),
        CheckConstraint(
            "proactive_reply_multiplier >= 1",
            name="ck_groups_proactive_reply_multiplier_min",
        ),
        CheckConstraint(
            "agent_free_mention_max_dispatches >= 0",
            name="ck_groups_agent_free_mention_max_dispatches_min",
        ),
        CheckConstraint(
            "communication_mode IN ('mesh', 'star', 'hierarchical', 'ring')",
            name="ck_groups_communication_mode",
        ),
    )

    owner_id: Mapped[UUID] = mapped_column(GUID(), nullable=False)
    workspace_id: Mapped[UUID | None] = mapped_column(
        GUID(), nullable=True, index=True
    )
    org_id: Mapped[UUID | None] = mapped_column(GUID(), nullable=True)
    name: Mapped[str] = mapped_column(String(100), nullable=False)
    avatar_url: Mapped[str | None] = mapped_column(Text, nullable=True)
    description: Mapped[str | None] = mapped_column(Text, nullable=True)
    group_type: Mapped[str | None] = mapped_column(String(50), nullable=True)
    announcement: Mapped[str | None] = mapped_column(Text, nullable=True)
    context_summary: Mapped[str | None] = mapped_column(Text, nullable=True)
    context_summary_message_id: Mapped[UUID | None] = mapped_column(
        GUID(), nullable=True
    )
    context_summary_updated_at: Mapped[datetime | None] = mapped_column(
        DateTime(timezone=True), nullable=True
    )
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
    proactive_reply_multiplier: Mapped[int] = mapped_column(
        Integer,
        default=1,
        server_default=text("1"),
        nullable=False,
    )
    allow_agent_free_mention: Mapped[bool] = mapped_column(
        Boolean, default=True, nullable=False
    )
    agent_free_mention_max_dispatches: Mapped[int] = mapped_column(
        Integer, default=8, server_default=text("8"), nullable=False
    )
    communication_mode: Mapped[str] = mapped_column(
        String(30), default="mesh", server_default=text("'mesh'"), nullable=False
    )
    muted_agent_ids: Mapped[list[str] | None] = mapped_column(JSONData, nullable=True)
    admin_agent_ids: Mapped[list[str] | None] = mapped_column(JSONData, nullable=True)
    muted_member_ids: Mapped[list[str] | None] = mapped_column(JSONData, nullable=True)
    status: Mapped[str] = mapped_column(String(30), default="active", nullable=False)
