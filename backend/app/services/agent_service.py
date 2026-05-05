from uuid import UUID

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.agents.builtin_tools import AgentToolConfig, normalize_tool_config
from app.core.exceptions import AgentChatError, NotFoundError, PermissionDeniedError
from app.models.agent import Agent
from app.models.user import User
from app.schemas.agent import AgentCreate, AgentUpdate
from app.services import llm_provider_service, skill_service, workspace_service


async def _validate_provider_and_skills(
    db: AsyncSession,
    owner: User,
    *,
    llm_provider_id: UUID | None,
    skill_ids: list[UUID] | None,
) -> None:
    """Owner-scope-check the optional provider and every skill referenced.

    Raises NotFoundError / PermissionDeniedError if any reference is invalid.
    """
    if llm_provider_id is not None:
        await llm_provider_service.get_provider(db, llm_provider_id, owner)
    if skill_ids:
        for sid in skill_ids:
            await skill_service.get_skill(db, sid, owner)


async def _validate_assistant_agents(
    db: AsyncSession,
    owner: User,
    tool_config: AgentToolConfig | None,
    *,
    self_agent_id: UUID | None = None,
) -> None:
    if tool_config is None:
        return
    for selection in tool_config.assistant_agents:
        if not selection.enabled:
            continue
        if self_agent_id is not None and selection.agent_id == self_agent_id:
            raise AgentChatError("agent cannot bind itself as an assistant tool")
        await get_agent(db, selection.agent_id, owner)


async def create_agent(db: AsyncSession, data: AgentCreate, owner: User) -> Agent:
    await _validate_provider_and_skills(
        db,
        owner,
        llm_provider_id=data.llm_provider_id,
        skill_ids=data.skill_ids,
    )
    await _validate_assistant_agents(db, owner, data.tool_config)
    await workspace_service.get_active_workspace(db, data.workspace_id, owner)
    agent = Agent(
        owner_id=owner.id,
        name=data.name,
        description=data.description,
        system_prompt=data.system_prompt,
        llm_config=data.llm_config,
        tool_config=normalize_tool_config(data.tool_config),
        workspace_id=data.workspace_id,
        llm_provider_id=data.llm_provider_id,
        skill_ids=[str(s) for s in data.skill_ids],
    )
    db.add(agent)
    await db.flush()
    await db.refresh(agent)
    return agent


async def list_agents(db: AsyncSession, owner: User) -> list[Agent]:
    result = await db.scalars(
        select(Agent)
        .where(Agent.owner_id == owner.id, Agent.status == "active")
        .order_by(Agent.created_at.desc())
    )
    return list(result)


async def get_agent(db: AsyncSession, agent_id: UUID, owner: User) -> Agent:
    agent = await db.scalar(select(Agent).where(Agent.id == agent_id))
    if agent is None or agent.status == "deleted":
        raise NotFoundError(f"agent {agent_id}")
    if agent.owner_id != owner.id:
        raise PermissionDeniedError("agent not accessible")
    return agent


async def update_agent(
    db: AsyncSession,
    agent_id: UUID,
    data: AgentUpdate,
    owner: User,
) -> Agent:
    agent = await get_agent(db, agent_id, owner)
    await _validate_provider_and_skills(
        db,
        owner,
        llm_provider_id=data.llm_provider_id,
        skill_ids=data.skill_ids,
    )
    if data.name is not None:
        agent.name = data.name
    if data.description is not None:
        agent.description = data.description
    if data.system_prompt is not None:
        agent.system_prompt = data.system_prompt
    if data.llm_config is not None:
        agent.llm_config = data.llm_config
    if data.tool_config is not None:
        await _validate_assistant_agents(db, owner, data.tool_config, self_agent_id=agent.id)
        agent.tool_config = normalize_tool_config(data.tool_config)
    if data.workspace_id is not None:
        await workspace_service.get_active_workspace(db, data.workspace_id, owner)
        agent.workspace_id = data.workspace_id
    # llm_provider_id: explicit None means "clear it"; we use a sentinel
    # check via the model_fields_set machinery.
    if "llm_provider_id" in data.model_fields_set:
        agent.llm_provider_id = data.llm_provider_id
    if data.skill_ids is not None:
        agent.skill_ids = [str(s) for s in data.skill_ids]
    await db.flush()
    await db.refresh(agent)
    return agent


async def delete_agent(db: AsyncSession, agent_id: UUID, owner: User) -> None:
    agent = await get_agent(db, agent_id, owner)
    agent.status = "deleted"
    await db.flush()
