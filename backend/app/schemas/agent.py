from datetime import datetime
from math import isfinite
from typing import Any, Literal
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field, field_validator

from app.agents.builtin_tools import AgentToolConfig, BuiltinToolRead
from app.agents.defaults import DEFAULT_AGENT_SYSTEM_PROMPT

AgentRuntimeKind = Literal["llm_chat", "external_cli"]
ExternalRuntimeAdapter = Literal["codex", "claude_code"]
TEMPERATURE_STEP = 0.05
TEMPERATURE_MIN = 0.0
TEMPERATURE_MAX = 2.0
TEMPERATURE_TOLERANCE = 1e-9


def _validate_temperature_step(value: Any) -> None:
    if isinstance(value, bool) or not isinstance(value, int | float):
        raise ValueError("llm_config.temperature must be a number")
    temperature = float(value)
    if not isfinite(temperature):
        raise ValueError("llm_config.temperature must be finite")
    if temperature < TEMPERATURE_MIN or temperature > TEMPERATURE_MAX:
        raise ValueError("llm_config.temperature must be between 0 and 2")
    step_count = temperature / TEMPERATURE_STEP
    if abs(step_count - round(step_count)) > TEMPERATURE_TOLERANCE:
        raise ValueError("llm_config.temperature must use 0.05 increments")


def _normalize_llm_config(value: dict[str, Any] | None) -> dict[str, Any] | None:
    if value is None:
        return value
    normalized = dict(value)
    normalized.pop("max_tokens", None)
    if "temperature" in normalized:
        _validate_temperature_step(normalized["temperature"])
    return normalized


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
    system_prompt: str = Field(default=DEFAULT_AGENT_SYSTEM_PROMPT, min_length=1)
    llm_config: dict[str, Any] | None = None
    tool_config: AgentToolConfig | None = None
    runtime_kind: AgentRuntimeKind = "llm_chat"
    external_runtime: ExternalRuntimeConfig | None = None
    workspace_id: UUID
    llm_provider_id: UUID | None = None
    skill_ids: list[UUID] = Field(default_factory=list)

    @field_validator("llm_config")
    @classmethod
    def validate_llm_config(cls, value: dict[str, Any] | None) -> dict[str, Any] | None:
        return _normalize_llm_config(value)


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

    @field_validator("llm_config")
    @classmethod
    def validate_llm_config(cls, value: dict[str, Any] | None) -> dict[str, Any] | None:
        return _normalize_llm_config(value)


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
