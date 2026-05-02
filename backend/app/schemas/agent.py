from datetime import datetime
from typing import Any
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field


class AgentCreate(BaseModel):
    name: str = Field(min_length=1, max_length=100)
    description: str | None = None
    system_prompt: str = Field(min_length=1)
    # Free-form bag: {"provider": "openai", "model": "...", "base_url": "...",
    # "api_key": "...", "temperature": 0.7}. Per-agent values override defaults
    # in app.core.config.settings.
    llm_config: dict[str, Any] | None = None


class AgentRead(BaseModel):
    model_config = ConfigDict(from_attributes=True)

    id: UUID
    name: str
    description: str | None
    system_prompt: str
    llm_config: dict[str, Any] | None = None
    visibility: str
    status: str
    created_at: datetime


class InvokeRequest(BaseModel):
    message: str = Field(min_length=1)


class InvokeResponse(BaseModel):
    content: str
