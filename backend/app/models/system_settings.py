from uuid import UUID

from sqlalchemy import Text
from sqlalchemy.dialects.postgresql import UUID as PgUUID
from sqlalchemy.orm import Mapped, mapped_column

from app.models.base import Base, TimestampMixin, UUIDPkMixin


class SystemSettings(Base, UUIDPkMixin, TimestampMixin):
    __tablename__ = "system_settings"

    owner_id: Mapped[UUID] = mapped_column(
        PgUUID(as_uuid=True), nullable=False, unique=True, index=True
    )
    group_workspace_root: Mapped[str | None] = mapped_column(Text, nullable=True)
