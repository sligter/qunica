from datetime import datetime
from typing import Any
from uuid import UUID

from sqlalchemy import DateTime, String, Text, func
from sqlalchemy.dialects.postgresql import JSONB
from sqlalchemy.dialects.postgresql import UUID as PgUUID
from sqlalchemy.orm import Mapped, mapped_column

from app.models.base import Base, UUIDPkMixin


class Message(Base, UUIDPkMixin):
    __tablename__ = "messages"

    group_id: Mapped[UUID] = mapped_column(PgUUID(as_uuid=True), nullable=False, index=True)
    thread_id: Mapped[UUID | None] = mapped_column(PgUUID(as_uuid=True), nullable=True)
    sender_type: Mapped[str] = mapped_column(String(20), nullable=False)  # user|agent|system
    sender_id: Mapped[UUID | None] = mapped_column(PgUUID(as_uuid=True), nullable=True)
    message_type: Mapped[str] = mapped_column(String(50), nullable=False)
    content: Mapped[str | None] = mapped_column(Text, nullable=True)
    content_json: Mapped[dict[str, Any] | None] = mapped_column(JSONB, nullable=True)
    # PRD §10.5 column name; renamed Python attr to avoid shadowing the
    # `references` builtin in nothing-major-but-safe-style.
    refs: Mapped[dict[str, Any] | None] = mapped_column("references", JSONB, nullable=True)
    reply_to_message_id: Mapped[UUID | None] = mapped_column(PgUUID(as_uuid=True), nullable=True)
    status: Mapped[str] = mapped_column(String(30), default="visible", nullable=False)
    # Use clock_timestamp() (statement-time) instead of now() (transaction-start)
    # so that user_msg + agent_msg persisted in the same `send_message_stream`
    # request transaction get distinct created_at values. See PRD
    # `05-05-fix-message-ordering-when-user-message-and-agent-reply-share-transaction-timestamp`.
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), server_default=func.clock_timestamp(), nullable=False
    )
