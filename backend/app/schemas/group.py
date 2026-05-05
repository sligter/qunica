from datetime import datetime
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field


class GroupCreate(BaseModel):
    name: str = Field(min_length=1, max_length=100)
    description: str | None = None
    announcement: str | None = None
    initial_agents: list[UUID] | None = None
    # Optional escape hatch: callers may pass an existing workspace to bind
    # instead of letting the service auto-create a dedicated one. The standard
    # path is omit-and-auto-create using the user's configured group workspace
    # root in system settings.
    workspace_id: UUID | None = None


class GroupUpdate(BaseModel):
    name: str | None = None
    description: str | None = None
    announcement: str | None = None
    free_speech: bool | None = None
    proactive_mode: bool | None = None
    proactive_max_rounds: int | None = Field(default=None, ge=1, le=5)
    allow_agent_free_mention: bool | None = None


class GroupMemberAdd(BaseModel):
    user_id: UUID


class GroupMemberRead(BaseModel):
    id: UUID
    group_id: UUID
    user_id: UUID
    display_name: str
    role: str
    status: str
    is_muted: bool
    joined_at: datetime


class GroupMemberMuteUpdate(BaseModel):
    muted: bool


class GroupAgentMuteUpdate(BaseModel):
    muted: bool


class GroupRead(BaseModel):
    model_config = ConfigDict(from_attributes=True)

    id: UUID
    workspace_id: UUID | None
    name: str
    description: str | None
    announcement: str | None
    free_speech: bool
    proactive_mode: bool
    proactive_max_rounds: int
    allow_agent_free_mention: bool
    muted_agent_ids: list[UUID] | None
    admin_agent_ids: list[UUID] | None
    muted_member_ids: list[UUID] | None
    status: str
    created_at: datetime


class GroupAgentAdd(BaseModel):
    agent_id: UUID
    share_group_workspace: bool = False


class GroupAgentWorkspaceSharingUpdate(BaseModel):
    share_group_workspace: bool


class ClearGroupMessagesResponse(BaseModel):
    cleared_count: int


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
    share_group_workspace: bool
    status: str
    joined_at: datetime
