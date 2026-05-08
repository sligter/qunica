from datetime import datetime
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field


class SystemSettingsRead(BaseModel):
    model_config = ConfigDict(from_attributes=True)

    id: UUID
    owner_id: UUID
    group_workspace_root: str | None
    web_search_provider: str
    tavily_api_key_configured: bool
    tavily_search_url: str
    tavily_max_results: int
    tavily_search_depth: str
    tavily_include_answer: bool
    tavily_include_raw_content: bool
    created_at: datetime
    updated_at: datetime


class SystemSettingsUpdate(BaseModel):
    """Patch payload for system settings.

    `group_workspace_root` may be:
    - omitted: keep existing value.
    - explicit `null`/empty string: clear the configured root.
    - non-empty string: set/replace the root path; service validates that the
      path resolves to an existing directory.
    """

    group_workspace_root: str | None = None
    web_search_provider: str | None = None
    tavily_api_key: str | None = None
    tavily_search_url: str | None = None
    tavily_max_results: int | None = Field(default=None, ge=1, le=20)
    tavily_search_depth: str | None = None
    tavily_include_answer: bool | None = None
    tavily_include_raw_content: bool | None = None
