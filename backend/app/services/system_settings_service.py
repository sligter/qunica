"""Per-user system settings (global preferences).

Currently exposes `group_workspace_root`, the directory under which group
workspaces are auto-created when a user creates a new group.
"""

from pathlib import Path

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.exceptions import AgentChatError
from app.models.system_settings import SystemSettings
from app.models.user import User
from app.schemas.system_settings import SystemSettingsUpdate


def _normalize_root(value: str | None) -> str | None:
    if value is None:
        return None
    stripped = value.strip()
    if not stripped:
        return None
    resolved = Path(stripped).expanduser().resolve()
    if not resolved.exists() or not resolved.is_dir():
        raise AgentChatError("group workspace root must be an existing directory")
    return str(resolved)


async def get_or_create(db: AsyncSession, owner: User) -> SystemSettings:
    existing = await db.scalar(
        select(SystemSettings).where(SystemSettings.owner_id == owner.id)
    )
    if existing is not None:
        return existing
    settings = SystemSettings(owner_id=owner.id)
    db.add(settings)
    await db.flush()
    await db.refresh(settings)
    return settings


async def update(
    db: AsyncSession, owner: User, data: SystemSettingsUpdate
) -> SystemSettings:
    settings = await get_or_create(db, owner)
    if "group_workspace_root" in data.model_fields_set:
        settings.group_workspace_root = _normalize_root(data.group_workspace_root)
    await db.flush()
    await db.refresh(settings)
    return settings


async def require_group_workspace_root(db: AsyncSession, owner: User) -> str:
    settings = await get_or_create(db, owner)
    if not settings.group_workspace_root:
        raise AgentChatError(
            "group workspace root is not configured; set it in system settings"
        )
    return settings.group_workspace_root
