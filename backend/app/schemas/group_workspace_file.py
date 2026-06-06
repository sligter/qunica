from datetime import datetime

from pydantic import BaseModel, Field


class GroupWorkspaceFileRead(BaseModel):
    path: str
    name: str
    is_dir: bool
    size: int | None = None
    modified_at: datetime | None = None
    abs_path: str | None = None


class GroupWorkspaceRoot(BaseModel):
    root: str
    separator: str


class GroupWorkspaceFilePreview(BaseModel):
    path: str
    name: str
    is_text: bool
    content: str | None = None
    truncated: bool = False
    message: str | None = None
    size: int | None = None


class GroupWorkspaceFileRename(BaseModel):
    new_path: str = Field(min_length=1, max_length=500)
