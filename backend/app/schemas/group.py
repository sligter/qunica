from datetime import datetime
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field


class GroupCreate(BaseModel):
    name: str = Field(min_length=1, max_length=100)
    description: str | None = None
    announcement: str | None = None
    initial_agents: list[UUID] | None = None


class GroupRead(BaseModel):
    model_config = ConfigDict(from_attributes=True)

    id: UUID
    name: str
    description: str | None
    announcement: str | None
    status: str
    created_at: datetime


class GroupAgentAdd(BaseModel):
    agent_id: UUID


class GroupAgentRead(BaseModel):
    """Resolved view of a group_agent row.

    `display_name` is computed: explicit `group_agents.display_name` if set,
    otherwise falls back to the underlying `agents.name`.
    """

    id: UUID
    group_id: UUID
    agent_id: UUID
    display_name: str
    role: str | None
    response_mode: str
    status: str
    joined_at: datetime
