from datetime import datetime
from math import isfinite
from typing import Any, Literal
from uuid import UUID

from pydantic import (
    BaseModel,
    ConfigDict,
    Field,
    field_validator,
    model_validator,
)

from app.agents.builtin_tools import AgentToolConfig, BuiltinToolRead
from app.agents.defaults import DEFAULT_AGENT_SYSTEM_PROMPT

AgentRuntimeKind = Literal["llm_chat", "acp"]
AcpRuntimeProfile = Literal["custom", "codex", "claude"]
AcpPermissionPolicy = Literal["deny", "auto_allow"]
AcpConfigValue = str | bool
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


class AcpRuntimeConfig(BaseModel):
    profile: AcpRuntimeProfile = "custom"
    command: str = Field(min_length=1)
    args: list[str] = Field(default_factory=list)
    env: dict[str, str] = Field(default_factory=dict)
    timeout_seconds: int | None = Field(default=3600, ge=1, le=21600)
    permission_policy: AcpPermissionPolicy = "deny"
    model: str | None = None
    mode: str | None = None
    thinking_effort: str | None = None
    config_options: dict[str, AcpConfigValue] | None = None


class AcpRuntimeChoice(BaseModel):
    value: str
    label: str
    description: str | None = None


class AcpRuntimePresetRead(BaseModel):
    id: Literal["codex", "claude"]
    name: str
    description: str
    profile: Literal["codex", "claude"]
    installed: bool
    command: str | None = None
    args: list[str] = Field(default_factory=list)
    env: dict[str, str] = Field(default_factory=dict)
    timeout_seconds: int = 3600
    permission_policy: AcpPermissionPolicy = "deny"
    default_model: str | None = None
    default_mode: str | None = None
    default_thinking_effort: str | None = None
    model_options: list[AcpRuntimeChoice] = Field(default_factory=list)
    mode_options: list[AcpRuntimeChoice] = Field(default_factory=list)
    thinking_effort_options: list[AcpRuntimeChoice] = Field(default_factory=list)
    install_hint: str
    source: str | None = None


class AcpRuntimePresetListResponse(BaseModel):
    presets: list[AcpRuntimePresetRead]


class AgentCreate(BaseModel):
    name: str = Field(min_length=1, max_length=100)
    description: str | None = None
    system_prompt: str = Field(default=DEFAULT_AGENT_SYSTEM_PROMPT, min_length=1)
    llm_config: dict[str, Any] | None = None
    tool_config: AgentToolConfig | None = None
    runtime_kind: AgentRuntimeKind = "llm_chat"
    acp_runtime: AcpRuntimeConfig | None = None
    workspace_id: UUID
    llm_provider_id: UUID | None = None
    skill_ids: list[UUID] = Field(default_factory=list)

    @model_validator(mode="before")
    @classmethod
    def reject_external_runtime(cls, value: Any) -> Any:
        if isinstance(value, dict) and "external_runtime" in value:
            raise ValueError("external_runtime is deprecated; use acp_runtime")
        return value

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
    acp_runtime: AcpRuntimeConfig | None = None
    workspace_id: UUID | None = None
    llm_provider_id: UUID | None = None
    skill_ids: list[UUID] | None = None

    @model_validator(mode="before")
    @classmethod
    def reject_external_runtime(cls, value: Any) -> Any:
        if isinstance(value, dict) and "external_runtime" in value:
            raise ValueError("external_runtime is deprecated; use acp_runtime")
        return value

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
    acp_runtime: AcpRuntimeConfig | None = Field(
        default=None,
        validation_alias="external_runtime",
    )
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
