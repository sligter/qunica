from datetime import datetime
from typing import Literal
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field

ProviderKind = Literal[
    "openai-compatible",
    "anthropic",
    "anthropic-compatible",
    "gemini",
]


class LLMProviderCreate(BaseModel):
    name: str = Field(min_length=1, max_length=100)
    kind: ProviderKind
    base_url: str | None = None
    api_key: str = Field(min_length=1)
    default_model: str = Field(min_length=1, max_length=200)
    context_window_tokens: int | None = Field(default=None, gt=0)
    context_output_reserve_ratio: float | None = Field(default=None, gt=0, lt=1)
    description: str | None = None
    reasoning_passback: bool = False


class LLMProviderUpdate(BaseModel):
    name: str | None = Field(default=None, min_length=1, max_length=100)
    kind: ProviderKind | None = None
    base_url: str | None = None
    api_key: str | None = Field(default=None, min_length=1)
    default_model: str | None = Field(default=None, min_length=1, max_length=200)
    context_window_tokens: int | None = Field(default=None, gt=0)
    context_output_reserve_ratio: float | None = Field(default=None, gt=0, lt=1)
    description: str | None = None
    reasoning_passback: bool | None = None


class LLMProviderRead(BaseModel):
    """Public read shape — `api_key` is masked (last 4 chars only)."""

    model_config = ConfigDict(from_attributes=True)

    id: UUID
    name: str
    kind: ProviderKind
    base_url: str | None
    api_key_masked: str
    default_model: str
    context_window_tokens: int | None
    context_output_reserve_ratio: float | None
    description: str | None
    reasoning_passback: bool
    status: str
    created_at: datetime
