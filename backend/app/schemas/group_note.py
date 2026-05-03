from datetime import datetime
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field


class GroupNoteCreate(BaseModel):
    title: str = Field(min_length=1, max_length=200)
    content: str = ""


class GroupNoteUpdate(BaseModel):
    title: str | None = None
    content: str | None = None


class GroupNoteRead(BaseModel):
    model_config = ConfigDict(from_attributes=True)

    id: UUID
    group_id: UUID
    title: str
    content: str
    created_at: datetime
    updated_at: datetime
