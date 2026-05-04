from datetime import datetime
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field


class LLMProviderCreate(BaseModel):
    name: str = Field(min_length=1, max_length=100)
    kind: str = Field(min_length=1, max_length=50)  # 'openai-compatible' | 'anthropic'
    base_url: str | None = None
    api_key: str = Field(min_length=1)
    default_model: str = Field(min_length=1, max_length=200)
    description: str | None = None


class LLMProviderUpdate(BaseModel):
    name: str | None = Field(default=None, min_length=1, max_length=100)
    kind: str | None = None
    base_url: str | None = None
    api_key: str | None = Field(default=None, min_length=1)
    default_model: str | None = Field(default=None, min_length=1, max_length=200)
    description: str | None = None


class LLMProviderRead(BaseModel):
    """Public read shape — `api_key` is masked (last 4 chars only)."""

    model_config = ConfigDict(from_attributes=True)

    id: UUID
    name: str
    kind: str
    base_url: str | None
    api_key_masked: str
    default_model: str
    description: str | None
    status: str
    created_at: datetime
