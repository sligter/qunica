from pathlib import Path
from uuid import UUID

from sqlalchemy import select, update
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.exceptions import AgentChatError, NotFoundError, PermissionDeniedError
from app.models.agent import Agent
from app.models.group import Group
from app.models.user import User
from app.models.workspace import Workspace
from app.schemas.workspace import WorkspaceCreate, WorkspaceUpdate

VALID_BACKEND_TYPES = ("local", "cloud_sandbox")


def _normalize_local_path(local_path: str | None) -> str | None:
    if local_path is None:
        return None
    resolved = Path(local_path).expanduser().resolve()
    if not resolved.exists() or not resolved.is_dir():
        raise AgentChatError("local workspace path must be an existing directory")
    return str(resolved)


def _validate_backend_type(backend_type: str) -> None:
    if backend_type not in VALID_BACKEND_TYPES:
        raise AgentChatError(f"unsupported workspace backend: {backend_type}")


def _normalize_workspace_input(
    *,
    backend_type: str,
    local_path: str | None,
) -> str | None:
    _validate_backend_type(backend_type)
    if backend_type == "local":
        if local_path is None:
            raise AgentChatError("local workspace requires local_path")
        return _normalize_local_path(local_path)
    return local_path


async def create_workspace(
    db: AsyncSession,
    data: WorkspaceCreate,
    owner: User,
) -> Workspace:
    normalized_path = _normalize_workspace_input(
        backend_type=data.backend_type,
        local_path=data.local_path,
    )
    workspace = Workspace(
        owner_id=owner.id,
        name=data.name,
        backend_type=data.backend_type,
        local_path=normalized_path,
        sandbox_ref=data.sandbox_ref,
        config=data.config,
    )
    db.add(workspace)
    await db.flush()
    await db.refresh(workspace)
    return workspace


async def list_workspaces(db: AsyncSession, owner: User) -> list[Workspace]:
    result = await db.scalars(
        select(Workspace)
        .where(Workspace.owner_id == owner.id, Workspace.status == "active")
        .order_by(Workspace.created_at.desc())
    )
    return list(result)


async def get_workspace(
    db: AsyncSession,
    workspace_id: UUID,
    owner: User,
) -> Workspace:
    workspace = await db.scalar(select(Workspace).where(Workspace.id == workspace_id))
    if workspace is None:
        raise NotFoundError(f"workspace {workspace_id}")
    if workspace.owner_id != owner.id:
        raise PermissionDeniedError("workspace not accessible")
    return workspace


async def update_workspace(
    db: AsyncSession,
    workspace_id: UUID,
    data: WorkspaceUpdate,
    owner: User,
) -> Workspace:
    workspace = await get_workspace(db, workspace_id, owner)
    backend_type = data.backend_type if data.backend_type is not None else workspace.backend_type
    local_path = data.local_path if "local_path" in data.model_fields_set else workspace.local_path
    normalized_path = _normalize_workspace_input(
        backend_type=backend_type,
        local_path=local_path,
    )

    if data.name is not None:
        workspace.name = data.name
    if data.backend_type is not None:
        workspace.backend_type = data.backend_type
    if "local_path" in data.model_fields_set or data.backend_type is not None:
        workspace.local_path = normalized_path
    if "sandbox_ref" in data.model_fields_set:
        workspace.sandbox_ref = data.sandbox_ref
    if "config" in data.model_fields_set:
        workspace.config = data.config

    await db.flush()
    await db.refresh(workspace)
    return workspace


async def delete_workspace(
    db: AsyncSession,
    workspace_id: UUID,
    owner: User,
) -> None:
    workspace = await get_workspace(db, workspace_id, owner)
    workspace.status = "deleted"
    await db.execute(
        update(Agent)
        .where(
            Agent.owner_id == owner.id,
            Agent.workspace_id == workspace_id,
            Agent.status == "active",
        )
        .values(workspace_id=None)
    )
    await db.execute(
        update(Group)
        .where(
            Group.owner_id == owner.id,
            Group.workspace_id == workspace_id,
            Group.status == "active",
        )
        .values(workspace_id=None)
    )
    await db.flush()


async def get_active_workspace(
    db: AsyncSession,
    workspace_id: UUID,
    owner: User,
) -> Workspace:
    workspace = await get_workspace(db, workspace_id, owner)
    if workspace.status != "active":
        raise NotFoundError(f"workspace {workspace_id}")
    return workspace
