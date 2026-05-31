from datetime import datetime
from uuid import UUID

from sqlalchemy import DateTime, String, UniqueConstraint, func
from sqlalchemy.orm import Mapped, mapped_column

from app.models.base import Base, UUIDPkMixin
from app.models.types import GUID


class GroupMember(Base, UUIDPkMixin):
    __tablename__ = "group_members"
    __table_args__ = (UniqueConstraint("group_id", "user_id", name="uq_group_members_group_user"),)

    group_id: Mapped[UUID] = mapped_column(GUID(), nullable=False)
    user_id: Mapped[UUID] = mapped_column(GUID(), nullable=False)
    role: Mapped[str] = mapped_column(String(30), default="member", nullable=False)
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
