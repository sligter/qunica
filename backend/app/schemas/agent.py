from datetime import datetime
from typing import Any, Literal
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field

from app.agents.builtin_tools import AgentToolConfig, BuiltinToolRead

AgentRuntimeKind = Literal["llm_chat", "external_cli"]
ExternalRuntimeAdapter = Literal["codex", "claude_code"]


class ExternalRuntimeConfig(BaseModel):
    adapter: ExternalRuntimeAdapter
    executable: str | None = None
    timeout_seconds: int | None = Field(default=3600, ge=1, le=21600)
    max_turns: int | None = Field(default=20, ge=1, le=100)


class ExternalAdapterStatusRead(BaseModel):
    adapter: ExternalRuntimeAdapter
    label: str
    executable: str
    configured_path: str | None
    resolved_path: str | None
    available: bool
    version: str | None = None
    error: str | None = None


class ExternalAdapterStatusResponse(BaseModel):
    adapters: list[ExternalAdapterStatusRead]


class AgentCreate(BaseModel):
    name: str = Field(min_length=1, max_length=100)
    description: str | None = None
    system_prompt: str = Field(min_length=1)
    llm_config: dict[str, Any] | None = None
    tool_config: AgentToolConfig | None = None
    runtime_kind: AgentRuntimeKind = "llm_chat"
    external_runtime: ExternalRuntimeConfig | None = None
    workspace_id: UUID
    llm_provider_id: UUID | None = None
    skill_ids: list[UUID] = Field(default_factory=list)


class AgentUpdate(BaseModel):
    name: str | None = Field(default=None, min_length=1, max_length=100)
    description: str | None = None
    system_prompt: str | None = Field(default=None, min_length=1)
    llm_config: dict[str, Any] | None = None
    tool_config: AgentToolConfig | None = None
    runtime_kind: AgentRuntimeKind | None = None
    external_runtime: ExternalRuntimeConfig | None = None
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
    runtime_kind: AgentRuntimeKind = "llm_chat"
    external_runtime: ExternalRuntimeConfig | None = None
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
