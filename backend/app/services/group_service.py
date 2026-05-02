from uuid import UUID

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.exceptions import ConflictError, NotFoundError, PermissionDeniedError
from app.models.agent import Agent
from app.models.group import Group
from app.models.group_agent import GroupAgent
from app.models.group_member import GroupMember
from app.models.user import User
from app.schemas.group import GroupCreate


async def create_group(db: AsyncSession, data: GroupCreate, owner: User) -> Group:
    group = Group(
        owner_id=owner.id,
        name=data.name,
        description=data.description,
        announcement=data.announcement,
    )
    db.add(group)
    await db.flush()  # need group.id for membership + agents

    db.add(GroupMember(group_id=group.id, user_id=owner.id, role="owner"))

    if data.initial_agents:
        for agent_id in data.initial_agents:
            agent = await db.scalar(select(Agent).where(Agent.id == agent_id))
            if agent is None:
                raise NotFoundError(f"agent {agent_id}")
            if agent.owner_id != owner.id:
                raise PermissionDeniedError("agent not accessible")
            db.add(
                GroupAgent(
                    group_id=group.id,
                    agent_id=agent_id,
                    response_mode="mentioned_only",
                )
            )

    await db.flush()
    await db.refresh(group)
    return group


async def list_groups_for_user(db: AsyncSession, user: User) -> list[Group]:
    stmt = (
        select(Group)
        .join(GroupMember, GroupMember.group_id == Group.id)
        .where(GroupMember.user_id == user.id, GroupMember.status == "active")
        .order_by(Group.created_at.desc())
    )
    return list(await db.scalars(stmt))


async def get_group(db: AsyncSession, group_id: UUID, user: User) -> Group:
    group = await db.scalar(select(Group).where(Group.id == group_id))
    if group is None:
        raise NotFoundError(f"group {group_id}")
    membership = await db.scalar(
        select(GroupMember).where(
            GroupMember.group_id == group_id,
            GroupMember.user_id == user.id,
            GroupMember.status == "active",
        )
    )
    if membership is None:
        raise PermissionDeniedError("not a member of this group")
    return group


async def assert_owner(db: AsyncSession, group_id: UUID, user: User) -> None:
    membership = await db.scalar(
        select(GroupMember).where(
            GroupMember.group_id == group_id,
            GroupMember.user_id == user.id,
        )
    )
    if membership is None or membership.role != "owner":
        raise PermissionDeniedError("only group owner can perform this action")


async def add_agent(
    db: AsyncSession, group_id: UUID, agent_id: UUID, owner: User
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
        raise ConflictError("agent already in group")

    ga = GroupAgent(
        group_id=group_id, agent_id=agent_id, response_mode="mentioned_only"
    )
    db.add(ga)
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
