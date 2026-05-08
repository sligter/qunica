from uuid import UUID

from sqlalchemy import Boolean, Integer, String, Text
from sqlalchemy.dialects.postgresql import UUID as PgUUID
from sqlalchemy.orm import Mapped, mapped_column

from app.models.base import Base, TimestampMixin, UUIDPkMixin


class SystemSettings(Base, UUIDPkMixin, TimestampMixin):
    __tablename__ = "system_settings"

    owner_id: Mapped[UUID] = mapped_column(
        PgUUID(as_uuid=True), nullable=False, unique=True, index=True
    )
    group_workspace_root: Mapped[str | None] = mapped_column(Text, nullable=True)
    web_search_provider: Mapped[str] = mapped_column(
        String(30), nullable=False, default="tavily", server_default="tavily"
    )
    tavily_api_key: Mapped[str | None] = mapped_column(Text, nullable=True)
    tavily_search_url: Mapped[str] = mapped_column(
        Text, nullable=False, default="https://api.tavily.com/search", server_default="https://api.tavily.com/search"
    )
    tavily_max_results: Mapped[int] = mapped_column(
        Integer, nullable=False, default=5, server_default="5"
    )
    tavily_search_depth: Mapped[str] = mapped_column(
        String(20), nullable=False, default="basic", server_default="basic"
    )
    tavily_include_answer: Mapped[bool] = mapped_column(
        Boolean, nullable=False, default=True, server_default="true"
    )
    tavily_include_raw_content: Mapped[bool] = mapped_column(
        Boolean, nullable=False, default=False, server_default="false"
    )

    @property
    def tavily_api_key_configured(self) -> bool:
        return bool(self.tavily_api_key)
