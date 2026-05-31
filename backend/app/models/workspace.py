from typing import Any
from uuid import UUID

from sqlalchemy import String, Text
from sqlalchemy.orm import Mapped, mapped_column

from app.models.base import Base, TimestampMixin, UUIDPkMixin
from app.models.types import GUID, JSONData


class Workspace(Base, UUIDPkMixin, TimestampMixin):
    __tablename__ = "workspaces"

    owner_id: Mapped[UUID] = mapped_column(GUID(), nullable=False)
    org_id: Mapped[UUID | None] = mapped_column(GUID(), nullable=True)
    name: Mapped[str] = mapped_column(String(100), nullable=False)
    backend_type: Mapped[str] = mapped_column(String(30), default="local", nullable=False)
    local_path: Mapped[str | None] = mapped_column(Text, nullable=True)
    sandbox_ref: Mapped[str | None] = mapped_column(Text, nullable=True)
    config: Mapped[dict[str, Any] | None] = mapped_column(JSONData, nullable=True)
    status: Mapped[str] = mapped_column(String(30), default="active", nullable=False)
