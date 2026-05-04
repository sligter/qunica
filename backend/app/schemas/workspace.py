from datetime import datetime
from typing import Any
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field


class WorkspaceCreate(BaseModel):
    name: str = Field(min_length=1, max_length=100)
    backend_type: str = Field(default="local", min_length=1, max_length=30)
    local_path: str | None = None
    sandbox_ref: str | None = None
    config: dict[str, Any] | None = None


class WorkspaceUpdate(BaseModel):
    name: str | None = Field(default=None, min_length=1, max_length=100)
    backend_type: str | None = Field(default=None, min_length=1, max_length=30)
    local_path: str | None = None
    sandbox_ref: str | None = None
    config: dict[str, Any] | None = None


class WorkspaceRead(BaseModel):
    model_config = ConfigDict(from_attributes=True)

    id: UUID
    name: str
    backend_type: str
    local_path: str | None
    sandbox_ref: str | None
    config: dict[str, Any] | None
    status: str
    created_at: datetime
    updated_at: datetime
