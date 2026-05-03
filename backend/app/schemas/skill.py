from datetime import datetime
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field


class SkillCreate(BaseModel):
    """Manual create — caller provides parsed fields directly."""

    name: str = Field(min_length=1, max_length=100)
    description: str | None = None
    body_markdown: str = Field(min_length=1)


class SkillImport(BaseModel):
    """Import from a SKILL.md raw text. Frontmatter parsed server-side."""

    raw: str = Field(min_length=1)


class SkillFileInfo(BaseModel):
    path: str
    size: int


class SkillRead(BaseModel):
    model_config = ConfigDict(from_attributes=True)

    id: UUID
    name: str
    description: str | None
    body_markdown: str
    source: str
    files: list[SkillFileInfo] | None = None
    storage_path: str | None = None
    status: str
    created_at: datetime
