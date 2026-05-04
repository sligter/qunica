from datetime import datetime
from typing import Any
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field

from app.agents.builtin_tools import AgentToolConfig, BuiltinToolRead


class AgentCreate(BaseModel):
    name: str = Field(min_length=1, max_length=100)
    description: str | None = None
    system_prompt: str = Field(min_length=1)
    llm_config: dict[str, Any] | None = None
    tool_config: AgentToolConfig | None = None
    workspace_id: UUID
    llm_provider_id: UUID | None = None
    skill_ids: list[UUID] = Field(default_factory=list)


class AgentUpdate(BaseModel):
    name: str | None = Field(default=None, min_length=1, max_length=100)
    description: str | None = None
    system_prompt: str | None = Field(default=None, min_length=1)
    llm_config: dict[str, Any] | None = None
    tool_config: AgentToolConfig | None = None
    workspace_id: UUID | None = None
    llm_provider_id: UUID | None = None
    skill_ids: list[UUID] | None = None


class AgentRead(BaseModel):
    model_config = ConfigDict(from_attributes=True)

    id: UUID
    name: str
    description: str | None
    system_prompt: str
    llm_config: dict[str, Any] | None = None
    tool_config: AgentToolConfig | None = None
    workspace_id: UUID | None = None
    llm_provider_id: UUID | None = None
    skill_ids: list[UUID] = Field(default_factory=list)
    visibility: str
    status: str
    created_at: datetime


class ToolCatalogResponse(BaseModel):
    tools: list[BuiltinToolRead]


class InvokeRequest(BaseModel):
    message: str = Field(min_length=1)


class InvokeResponse(BaseModel):
    content: str
