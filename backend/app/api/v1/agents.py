from collections.abc import AsyncIterator
from typing import Any, cast
from uuid import UUID

from fastapi import APIRouter, Depends, status
from langchain_core.messages import AIMessage, BaseMessage, HumanMessage, ToolMessage
from sqlalchemy.ext.asyncio import AsyncSession
from sse_starlette.sse import EventSourceResponse

from app.agents.builtin_tools import list_builtin_tools
from app.agents.context import build_agent_invocation_context
from app.agents.runtime import TOOL_LOOP_REPEATED_CALL_LIMIT
from app.agents.workspace_tools import bind_workspace_tools, build_workspace_tools
from app.core.deps import get_current_user
from app.core.exceptions import LLMProviderError
from app.db import get_db
from app.llm.chat_model import resolve_chat_model
from app.models.agent import Agent
from app.models.user import User
from app.schemas.agent import (
    AgentCreate,
    AgentRead,
    AgentUpdate,
    InvokeRequest,
    InvokeResponse,
    ToolCatalogResponse,
)
from app.services import agent_service

router = APIRouter(prefix="/agents", tags=["agents"])


def _build_messages(system_message: BaseMessage, user_message: str) -> list[BaseMessage]:
    return [system_message, HumanMessage(content=user_message)]


def _direct_tool_signature(tool_call: dict[str, object]) -> tuple[str, str]:
    name = str(tool_call.get("name") or "")
    args = tool_call.get("args")
    args_signature = repr(sorted(args.items())) if isinstance(args, dict) else repr(args)
    return (name, args_signature)


async def _invoke_with_tool_loop(
    model: Any,
    tools: dict[str, Any],
    messages: list[BaseMessage],
) -> Any:
    repeated_call_counts: dict[tuple[str, str], int] = {}
    while True:
        response = await model.ainvoke(messages)
        if not isinstance(response, AIMessage) or not response.tool_calls:
            return response
        for tool_call in response.tool_calls:
            signature = _direct_tool_signature(cast(dict[str, object], tool_call))
            repeated_call_counts[signature] = repeated_call_counts.get(signature, 0) + 1
            if repeated_call_counts[signature] > TOOL_LOOP_REPEATED_CALL_LIMIT:
                return AIMessage(
                    content=(
                        f"Tool execution paused because the model repeatedly requested the same "
                        f"{signature[0] or 'unknown'} tool call without making progress. "
                        "Summarize the completed tool results and ask the user how to proceed."
                    )
                )
        messages.append(response)
        for index, tool_call in enumerate(response.tool_calls):
            tool_call_dict = cast(dict[str, object], tool_call)
            name = str(tool_call_dict.get("name") or "")
            args = tool_call_dict.get("args")
            call_id = str(tool_call_dict.get("id") or f"tool-call-{index}")
            tool_args = cast(dict[str, Any], args) if isinstance(args, dict) else {}
            executor = tools.get(name)
            result = (
                f"Tool {name!r} is unavailable in this runtime."
                if executor is None
                else str(await executor.ainvoke(tool_args))
            )
            messages.append(ToolMessage(content=result, tool_call_id=call_id))


@router.get("/tool-catalog", response_model=ToolCatalogResponse)
async def get_tool_catalog() -> ToolCatalogResponse:
    return ToolCatalogResponse(tools=list_builtin_tools())


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


@router.patch("/{agent_id}", response_model=AgentRead)
async def update_agent(
    agent_id: UUID,
    data: AgentUpdate,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> Agent:
    return await agent_service.update_agent(db, agent_id, data, current_user)


@router.delete("/{agent_id}", status_code=status.HTTP_204_NO_CONTENT)
async def delete_agent(
    agent_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> None:
    await agent_service.delete_agent(db, agent_id, current_user)


@router.post("/{agent_id}/invoke", response_model=InvokeResponse)
async def invoke_agent(
    agent_id: UUID,
    data: InvokeRequest,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> InvokeResponse:
    agent = await agent_service.get_agent(db, agent_id, current_user)
    context = await build_agent_invocation_context(db, agent, current_user)
    chat_model = await resolve_chat_model(db, agent, streaming=False)
    tools = build_workspace_tools(context)
    model = bind_workspace_tools(chat_model, tools)
    messages = _build_messages(context.to_system_message(), data.message)
    try:
        response = await _invoke_with_tool_loop(model, tools, messages)
    except Exception as exc:
        raise LLMProviderError(f"chat_complete failed: {exc}") from exc
    content = (
        response.content
        if isinstance(response, AIMessage)
        else str(getattr(response, "content", response))
    )
    return InvokeResponse(content=cast(str, content))


@router.post("/{agent_id}/invoke/stream")
async def invoke_agent_stream(
    agent_id: UUID,
    data: InvokeRequest,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> EventSourceResponse:
    agent = await agent_service.get_agent(db, agent_id, current_user)
    context = await build_agent_invocation_context(db, agent, current_user)
    chat_model = await resolve_chat_model(db, agent, streaming=True)
    tools = build_workspace_tools(context)

    async def event_gen() -> AsyncIterator[dict[str, str]]:
        try:
            if tools:
                model = bind_workspace_tools(chat_model, tools)
                messages = _build_messages(context.to_system_message(), data.message)
                response = await _invoke_with_tool_loop(model, tools, messages)
                content = (
                    response.content
                    if isinstance(response, AIMessage)
                    else str(getattr(response, "content", response))
                )
                if isinstance(content, str) and content:
                    yield {"event": "token", "data": content}
            else:
                async for chunk in chat_model.astream(
                    _build_messages(context.to_system_message(), data.message)
                ):
                    chunk_content = getattr(chunk, "content", None)
                    if isinstance(chunk_content, str) and chunk_content:
                        yield {"event": "token", "data": chunk_content}
        except Exception as exc:
            yield {"event": "error", "data": f"chat_stream failed: {exc}"}
            return
        yield {"event": "done", "data": ""}

    return EventSourceResponse(event_gen())
