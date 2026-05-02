from uuid import UUID

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.exceptions import NotFoundError, PermissionDeniedError
from app.models.agent import Agent
from app.models.user import User
from app.schemas.agent import AgentCreate


async def create_agent(db: AsyncSession, data: AgentCreate, owner: User) -> Agent:
    agent = Agent(
        owner_id=owner.id,
        name=data.name,
        description=data.description,
        system_prompt=data.system_prompt,
        llm_config=data.llm_config,
    )
    db.add(agent)
    await db.flush()
    await db.refresh(agent)
    return agent


async def list_agents(db: AsyncSession, owner: User) -> list[Agent]:
    result = await db.scalars(
        select(Agent)
        .where(Agent.owner_id == owner.id)
        .order_by(Agent.created_at.desc())
    )
    return list(result)


async def get_agent(db: AsyncSession, agent_id: UUID, owner: User) -> Agent:
    agent = await db.scalar(select(Agent).where(Agent.id == agent_id))
    if agent is None:
        raise NotFoundError(f"agent {agent_id}")
    if agent.owner_id != owner.id:
        raise PermissionDeniedError("agent not accessible")
    return agent
