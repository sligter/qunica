from datetime import datetime
from uuid import UUID

from pydantic import BaseModel, ConfigDict


class SystemSettingsRead(BaseModel):
    model_config = ConfigDict(from_attributes=True)

    id: UUID
    owner_id: UUID
    group_workspace_root: str | None
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
