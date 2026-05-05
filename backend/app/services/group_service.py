from datetime import timedelta
from pathlib import Path
from uuid import UUID

from sqlalchemy import or_, select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.exceptions import (
    AgentChatError,
    ConflictError,
    NotFoundError,
    PermissionDeniedError,
)
from app.models.agent import Agent
from app.models.group import Group
from app.models.group_agent import GroupAgent
from app.models.group_member import GroupMember
from app.models.user import User
from app.models.workspace import Workspace
from app.schemas.group import GroupCreate, GroupUpdate
from app.services import system_settings_service, workspace_service


async def _get_membership(
    db: AsyncSession,
    group_id: UUID,
    user_id: UUID,
    *,
    active_only: bool = False,
) -> GroupMember | None:
    conditions = [
        GroupMember.group_id == group_id,
        GroupMember.user_id == user_id,
    ]
    if active_only:
        conditions.append(GroupMember.status == "active")
    membership: GroupMember | None = await db.scalar(select(GroupMember).where(*conditions))
    return membership


def _is_group_owner(membership: GroupMember | None) -> bool:
    return membership is not None and membership.role == "owner" and membership.status == "active"


def _add_uuid_to_json_list(values: list[str] | None, item_id: UUID) -> list[str]:
    item = str(item_id)
    current = list(values or [])
    if item not in current:
        current.append(item)
    return current


def _remove_uuid_from_json_list(values: list[str] | None, item_id: UUID) -> list[str]:
    item = str(item_id)
    return [value for value in values or [] if value != item]


def _set_group_workspace_sharing(ga: GroupAgent, share: bool) -> None:
    context_scope = dict(ga.context_scope or {})
    file_scope = dict(ga.file_scope or {})
    if share:
        context_scope["share_group_workspace"] = True
        file_scope["group_workspace"] = "read"
    else:
        context_scope.pop("share_group_workspace", None)
        file_scope.pop("group_workspace", None)
    ga.context_scope = context_scope or None
    ga.file_scope = file_scope or None


def is_group_workspace_shared(ga: GroupAgent | None) -> bool:
    if ga is None:
        return False
    context_scope = ga.context_scope or {}
    return context_scope.get("share_group_workspace") is True


async def _bind_or_create_group_workspace(
    db: AsyncSession, group: Group, owner: User, requested_workspace_id: UUID | None
) -> Workspace:
    """Bind the group to a dedicated workspace.

    Default flow auto-creates a workspace under the user's configured
    `group_workspace_root` in system settings. Callers may pass
    `requested_workspace_id` to bind an existing workspace they own; this is an
    escape hatch and not the standard path.
    """
    if requested_workspace_id is not None:
        workspace = await workspace_service.get_active_workspace(
            db, requested_workspace_id, owner
        )
        return workspace

    root = await system_settings_service.require_group_workspace_root(db, owner)
    storage_dir = Path(root) / str(group.id)
    try:
        storage_dir.mkdir(parents=True, exist_ok=True)
    except OSError as exc:
        raise AgentChatError(
            f"failed to create group workspace directory: {exc}"
        ) from exc
    resolved = storage_dir.resolve()
    workspace = Workspace(
        owner_id=owner.id,
        name=f"group:{group.name}",
        backend_type="local",
        local_path=str(resolved),
    )
    db.add(workspace)
    await db.flush()
    await db.refresh(workspace)
    return workspace


async def create_group(db: AsyncSession, data: GroupCreate, owner: User) -> Group:
    group = Group(
        owner_id=owner.id,
        name=data.name,
        description=data.description,
        announcement=data.announcement,
    )
    db.add(group)
    await db.flush()  # need group.id for workspace path + membership + agents

    workspace = await _bind_or_create_group_workspace(
        db, group, owner, data.workspace_id
    )
    group.workspace_id = workspace.id

    db.add(GroupMember(group_id=group.id, user_id=owner.id, role="owner"))

    if data.initial_agents:
        for position, agent_id in enumerate(data.initial_agents):
            agent = await db.scalar(select(Agent).where(Agent.id == agent_id))
            if agent is None:
                raise NotFoundError(f"agent {agent_id}")
            if agent.owner_id != owner.id:
                raise PermissionDeniedError("agent not accessible")
            ga = GroupAgent(
                group_id=group.id,
                agent_id=agent_id,
                response_mode="mentioned_only",
            )
            ga.joined_at = group.created_at + timedelta(microseconds=position)
            if agent.workspace_id == workspace.id:
                _set_group_workspace_sharing(ga, True)
            db.add(ga)

    await db.flush()
    await db.refresh(group)
    return group


async def list_groups_for_user(db: AsyncSession, user: User) -> list[Group]:
    stmt = (
        select(Group)
        .join(GroupMember, GroupMember.group_id == Group.id)
        .where(
            GroupMember.user_id == user.id,
            GroupMember.status == "active",
            Group.status == "active",
        )
        .order_by(Group.created_at.desc())
    )
    return list(await db.scalars(stmt))


async def get_group(db: AsyncSession, group_id: UUID, user: User) -> Group:
    group = await db.scalar(select(Group).where(Group.id == group_id))
    if group is None:
        raise NotFoundError(f"group {group_id}")
    membership = await _get_membership(db, group_id, user.id, active_only=True)
    if membership is None:
        raise PermissionDeniedError("not a member of this group")
    return group


async def update_group(
    db: AsyncSession, group_id: UUID, data: GroupUpdate, user: User
) -> Group:
    group = await get_group(db, group_id, user)
    # Only owner can update settings
    membership = await _get_membership(db, group_id, user.id)
    if not _is_group_owner(membership):
        raise PermissionDeniedError("only group owner can update settings")

    if data.name is not None:
        group.name = data.name
    if data.description is not None:
        group.description = data.description
    if data.announcement is not None:
        group.announcement = data.announcement
    if data.free_speech is not None:
        group.free_speech = data.free_speech
    if data.proactive_mode is not None:
        group.proactive_mode = data.proactive_mode
    if data.proactive_max_rounds is not None:
        group.proactive_max_rounds = data.proactive_max_rounds
    if data.proactive_reply_multiplier is not None:
        group.proactive_reply_multiplier = data.proactive_reply_multiplier
    if data.allow_agent_free_mention is not None:
        group.allow_agent_free_mention = data.allow_agent_free_mention

    await db.flush()
    await db.refresh(group)
    return group


async def assert_owner(db: AsyncSession, group_id: UUID, user: User) -> None:
    group = await db.scalar(select(Group).where(Group.id == group_id, Group.status == "active"))
    if group is None:
        raise NotFoundError(f"group {group_id}")
    membership = await _get_membership(db, group_id, user.id)
    if not _is_group_owner(membership):
        raise PermissionDeniedError("only group owner can perform this action")


async def delete_group(db: AsyncSession, group_id: UUID, user: User) -> None:
    """Soft-delete: flip status to 'deleted'. Membership rows preserved."""
    group = await get_group(db, group_id, user)
    await assert_owner(db, group_id, user)
    group.status = "deleted"
    await db.flush()


async def add_agent(
    db: AsyncSession,
    group_id: UUID,
    agent_id: UUID,
    owner: User,
    *,
    share_group_workspace: bool = False,
) -> tuple[GroupAgent, Agent]:
    await assert_owner(db, group_id, owner)

    agent = await db.scalar(select(Agent).where(Agent.id == agent_id))
    if agent is None:
        raise NotFoundError(f"agent {agent_id}")
    if agent.owner_id != owner.id:
        raise PermissionDeniedError("agent not accessible")

    existing = await db.scalar(
        select(GroupAgent).where(
            GroupAgent.group_id == group_id,
            GroupAgent.agent_id == agent_id,
        )
    )
    if existing is not None:
        if existing.status == "active":
            raise ConflictError("agent already in group")
        existing.status = "active"
        ga = existing
    else:
        ga = GroupAgent(
            group_id=group_id, agent_id=agent_id, response_mode="mentioned_only"
        )
        db.add(ga)
    _set_group_workspace_sharing(ga, share_group_workspace)
    await db.flush()
    await db.refresh(ga)
    return ga, agent


async def list_agents_in_group(
    db: AsyncSession, group_id: UUID, user: User
) -> list[tuple[GroupAgent, Agent]]:
    await get_group(db, group_id, user)
    stmt = (
        select(GroupAgent, Agent)
        .join(Agent, Agent.id == GroupAgent.agent_id)
        .where(GroupAgent.group_id == group_id, GroupAgent.status == "active")
        .order_by(GroupAgent.joined_at.asc())
    )
    rows = (await db.execute(stmt)).all()
    return [(row[0], row[1]) for row in rows]


async def set_agent_workspace_sharing(
    db: AsyncSession,
    group_id: UUID,
    agent_id: UUID,
    share_group_workspace: bool,
    user: User,
) -> tuple[GroupAgent, Agent]:
    await assert_owner(db, group_id, user)
    row = await db.execute(
        select(GroupAgent, Agent)
        .join(Agent, Agent.id == GroupAgent.agent_id)
        .where(
            GroupAgent.group_id == group_id,
            GroupAgent.agent_id == agent_id,
            GroupAgent.status == "active",
        )
    )
    result = row.one_or_none()
    if result is None:
        raise NotFoundError(f"group agent {agent_id}")
    _set_group_workspace_sharing(result[0], share_group_workspace)
    await db.flush()
    await db.refresh(result[0])
    return result[0], result[1]


async def remove_agent(db: AsyncSession, group_id: UUID, agent_id: UUID, user: User) -> None:
    await assert_owner(db, group_id, user)
    group_agent = await db.scalar(
        select(GroupAgent).where(
            GroupAgent.group_id == group_id,
            GroupAgent.agent_id == agent_id,
            GroupAgent.status == "active",
        )
    )
    if group_agent is None:
        raise NotFoundError(f"group agent {agent_id}")
    group_agent.status = "removed"
    group = await db.scalar(select(Group).where(Group.id == group_id))
    if group is not None:
        group.muted_agent_ids = _remove_uuid_from_json_list(group.muted_agent_ids, agent_id)
        group.admin_agent_ids = _remove_uuid_from_json_list(group.admin_agent_ids, agent_id)
    await db.flush()


async def set_agent_muted(
    db: AsyncSession, group_id: UUID, agent_id: UUID, muted: bool, user: User
) -> tuple[GroupAgent, Agent, Group]:
    await assert_owner(db, group_id, user)
    group = await db.scalar(select(Group).where(Group.id == group_id, Group.status == "active"))
    if group is None:
        raise NotFoundError(f"group {group_id}")
    row = await db.execute(
        select(GroupAgent, Agent)
        .join(Agent, Agent.id == GroupAgent.agent_id)
        .where(
            GroupAgent.group_id == group_id,
            GroupAgent.agent_id == agent_id,
            GroupAgent.status == "active",
        )
    )
    result = row.one_or_none()
    if result is None:
        raise NotFoundError(f"group agent {agent_id}")
    if muted:
        group.muted_agent_ids = _add_uuid_to_json_list(group.muted_agent_ids, agent_id)
    else:
        group.muted_agent_ids = _remove_uuid_from_json_list(group.muted_agent_ids, agent_id)
    await db.flush()
    await db.refresh(group)
    return result[0], result[1], group


async def list_members_in_group(
    db: AsyncSession, group_id: UUID, user: User
) -> list[tuple[GroupMember, User]]:
    await get_group(db, group_id, user)
    stmt = (
        select(GroupMember, User)
        .join(User, User.id == GroupMember.user_id)
        .where(GroupMember.group_id == group_id, GroupMember.status == "active")
        .order_by(GroupMember.joined_at.asc())
    )
    rows = (await db.execute(stmt)).all()
    return [(row[0], row[1]) for row in rows]


async def search_users_for_group(
    db: AsyncSession, group_id: UUID, query: str, user: User, limit: int = 20
) -> list[User]:
    await assert_owner(db, group_id, user)
    trimmed = query.strip()
    pattern = f"%{trimmed}%"
    stmt = select(User).order_by(User.created_at.desc()).limit(limit)
    if trimmed:
        stmt = stmt.where(or_(User.name.ilike(pattern), User.email.ilike(pattern)))
    return list(await db.scalars(stmt))


async def add_member(
    db: AsyncSession, group_id: UUID, user_id: UUID, owner: User
) -> tuple[GroupMember, User]:
    await assert_owner(db, group_id, owner)
    member_user = await db.scalar(select(User).where(User.id == user_id))
    if member_user is None:
        raise NotFoundError(f"user {user_id}")
    existing = await _get_membership(db, group_id, user_id)
    if existing is not None:
        if existing.status == "active":
            raise ConflictError("user already in group")
        existing.status = "active"
        existing.role = "member"
        member = existing
    else:
        member = GroupMember(group_id=group_id, user_id=user_id, role="member")
        db.add(member)
    await db.flush()
    await db.refresh(member)
    return member, member_user


async def remove_member(db: AsyncSession, group_id: UUID, user_id: UUID, user: User) -> None:
    await assert_owner(db, group_id, user)
    membership = await _get_membership(db, group_id, user_id, active_only=True)
    if membership is None:
        raise NotFoundError(f"group member {user_id}")
    if membership.role == "owner":
        raise PermissionDeniedError("group owner cannot be removed")
    membership.status = "removed"
    group = await db.scalar(select(Group).where(Group.id == group_id))
    if group is not None:
        group.muted_member_ids = _remove_uuid_from_json_list(group.muted_member_ids, user_id)
    await db.flush()


async def set_member_muted(
    db: AsyncSession, group_id: UUID, user_id: UUID, muted: bool, user: User
) -> tuple[GroupMember, User, Group]:
    await assert_owner(db, group_id, user)
    group = await db.scalar(select(Group).where(Group.id == group_id, Group.status == "active"))
    if group is None:
        raise NotFoundError(f"group {group_id}")
    membership = await _get_membership(db, group_id, user_id, active_only=True)
    if membership is None:
        raise NotFoundError(f"group member {user_id}")
    if membership.role == "owner":
        raise PermissionDeniedError("group owner cannot be muted")
    member_user = await db.scalar(select(User).where(User.id == user_id))
    if member_user is None:
        raise NotFoundError(f"user {user_id}")
    if muted:
        group.muted_member_ids = _add_uuid_to_json_list(group.muted_member_ids, user_id)
    else:
        group.muted_member_ids = _remove_uuid_from_json_list(group.muted_member_ids, user_id)
    await db.flush()
    await db.refresh(group)
    return membership, member_user, group
