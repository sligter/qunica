from typing import Any
from uuid import UUID

from sqlalchemy import String, Text
from sqlalchemy.dialects.postgresql import JSONB
from sqlalchemy.dialects.postgresql import UUID as PgUUID
from sqlalchemy.orm import Mapped, mapped_column

from app.models.base import Base, TimestampMixin, UUIDPkMixin


class Agent(Base, UUIDPkMixin, TimestampMixin):
    __tablename__ = "agents"

    owner_id: Mapped[UUID] = mapped_column(PgUUID(as_uuid=True), nullable=False)
    org_id: Mapped[UUID | None] = mapped_column(PgUUID(as_uuid=True), nullable=True)
    name: Mapped[str] = mapped_column(String(100), nullable=False)
    avatar_url: Mapped[str | None] = mapped_column(Text, nullable=True)
    description: Mapped[str | None] = mapped_column(Text, nullable=True)
    system_prompt: Mapped[str] = mapped_column(Text, nullable=False)
    # Python attribute renamed to avoid Pydantic's reserved `model_config`;
    # DB column name follows PRD §10.2.
    llm_config: Mapped[dict[str, Any] | None] = mapped_column(
        "model_config", JSONB, nullable=True
    )
    tool_config: Mapped[dict[str, Any] | None] = mapped_column(JSONB, nullable=True)
    memory_policy: Mapped[dict[str, Any] | None] = mapped_column(JSONB, nullable=True)
    visibility: Mapped[str] = mapped_column(String(30), default="private", nullable=False)
    status: Mapped[str] = mapped_column(String(30), default="active", nullable=False)
    workspace_id: Mapped[UUID | None] = mapped_column(
        PgUUID(as_uuid=True), nullable=True
    )
    llm_provider_id: Mapped[UUID | None] = mapped_column(
        PgUUID(as_uuid=True), nullable=True
    )
    skill_ids: Mapped[list[Any]] = mapped_column(
        JSONB, nullable=False, default=list, server_default="[]"
    )
