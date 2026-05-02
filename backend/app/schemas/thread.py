from datetime import datetime
from uuid import UUID

from pydantic import BaseModel, ConfigDict


class ThreadRead(BaseModel):
    model_config = ConfigDict(from_attributes=True)

    id: UUID
    group_id: UUID
    agent_id: UUID | None
    created_by: UUID | None
    thread_type: str | None
    title: str | None
    goal: str | None
    status: str | None
    priority: int
    started_at: datetime | None
    completed_at: datetime | None
    created_at: datetime
    updated_at: datetime
