from collections.abc import AsyncIterator
from typing import cast
from uuid import UUID

from fastapi import APIRouter, Depends, status
from langchain_core.messages import (
    AIMessage,
    BaseMessage,
    HumanMessage,
    SystemMessage,
)
from openai import OpenAIError
from sqlalchemy.ext.asyncio import AsyncSession
from sse_starlette.sse import EventSourceResponse

from app.core.deps import get_current_user
from app.core.exceptions import LLMProviderError
from app.db import get_db
from app.llm import make_chat_model
from app.models.agent import Agent
from app.models.user import User
from app.schemas.agent import (
    AgentCreate,
    AgentRead,
    InvokeRequest,
    InvokeResponse,
)
from app.services import agent_service

router = APIRouter(prefix="/agents", tags=["agents"])


def _build_messages(agent: Agent, user_message: str) -> list[BaseMessage]:
    return [
        SystemMessage(content=agent.system_prompt),
        HumanMessage(content=user_message),
    ]


@router.post(
    "",
    response_model=AgentRead,
    status_code=status.HTTP_201_CREATED,
)
async def create_agent(
    data: AgentCreate,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> Agent:
    return await agent_service.create_agent(db, data, current_user)


@router.get("", response_model=list[AgentRead])
async def list_agents(
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> list[Agent]:
    return await agent_service.list_agents(db, current_user)


@router.get("/{agent_id}", response_model=AgentRead)
async def get_agent(
    agent_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> Agent:
    return await agent_service.get_agent(db, agent_id, current_user)


@router.post("/{agent_id}/invoke", response_model=InvokeResponse)
async def invoke_agent(
    agent_id: UUID,
    data: InvokeRequest,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> InvokeResponse:
    agent = await agent_service.get_agent(db, agent_id, current_user)
    model = make_chat_model(agent.llm_config)
    try:
        response = await model.ainvoke(_build_messages(agent, data.message))
    except OpenAIError as exc:
        raise LLMProviderError(f"chat_complete failed: {exc}") from exc
    content = response.content if isinstance(response, AIMessage) else str(response.content)
    return InvokeResponse(content=cast(str, content))


@router.post("/{agent_id}/invoke/stream")
async def invoke_agent_stream(
    agent_id: UUID,
    data: InvokeRequest,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> EventSourceResponse:
    agent = await agent_service.get_agent(db, agent_id, current_user)
    model = make_chat_model(agent.llm_config)

    async def event_gen() -> AsyncIterator[dict[str, str]]:
        try:
            async for chunk in model.astream(_build_messages(agent, data.message)):
                content = getattr(chunk, "content", None)
                if isinstance(content, str) and content:
                    yield {"event": "token", "data": content}
        except OpenAIError as exc:
            yield {"event": "error", "data": f"chat_stream failed: {exc}"}
            return
        yield {"event": "done", "data": ""}

    return EventSourceResponse(event_gen())
