from datetime import datetime
from uuid import UUID

from sqlalchemy import Boolean, DateTime, String, Text, false, func
from sqlalchemy.orm import Mapped, mapped_column

from app.models.base import Base, UUIDPkMixin
from app.models.types import GUID


class LLMProvider(Base, UUIDPkMixin):
    __tablename__ = "llm_providers"

    owner_id: Mapped[UUID] = mapped_column(GUID(), nullable=False, index=True)
    name: Mapped[str] = mapped_column(String(100), nullable=False)
    # 'openai-compatible' | 'anthropic' | 'anthropic-compatible' | 'gemini'
    kind: Mapped[str] = mapped_column(String(50), nullable=False)
    base_url: Mapped[str | None] = mapped_column(Text, nullable=True)
    api_key: Mapped[str] = mapped_column(Text, nullable=False)
    default_model: Mapped[str] = mapped_column(String(200), nullable=False)
    description: Mapped[str | None] = mapped_column(Text, nullable=True)
    # Re-send the model's prior `reasoning_content` on follow-up turns of a
    # multi-turn tool loop. Reasoning models with tool use (e.g. DeepSeek, Xiaomi
    # MiMo) expect the thinking that produced a tool call to travel back with it.
    # Per-provider opt-in (default off); only honored by the openai-compatible path.
    reasoning_passback: Mapped[bool] = mapped_column(
        Boolean, nullable=False, default=False, server_default=false()
    )
    status: Mapped[str] = mapped_column(String(30), default="active", nullable=False)
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), server_default=func.now(), nullable=False
    )
    updated_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True),
        server_default=func.now(),
        onupdate=func.now(),
        nullable=False,
    )
