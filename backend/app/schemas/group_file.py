from datetime import datetime
from uuid import UUID

from pydantic import BaseModel, ConfigDict


class GroupFileRead(BaseModel):
    model_config = ConfigDict(from_attributes=True)

    id: UUID
    group_id: UUID
    filename: str
    file_size: int
    mime_type: str | None
    created_at: datetime
