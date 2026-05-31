import json
from collections.abc import AsyncIterator
from dataclasses import asdict
from pathlib import Path
from typing import Any, cast
from uuid import UUID

from fastapi import APIRouter, Depends, Request, status
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
from app.external_agents import (
    ADAPTER_LABELS,
    detect_adapter_status,
    normalize_external_runtime,
    run_external_agent,
    run_external_agent_stream,
)
from app.llm.chat_model import resolve_chat_model
from app.models.agent import Agent
from app.models.user import User
from app.schemas.agent import (
    AgentCreate,
    AgentRead,
    AgentUpdate,
    ExternalAdapterStatusRead,
    ExternalAdapterStatusResponse,
    InvokeRequest,
    InvokeResponse,
    ToolCatalogResponse,
)
from app.services import agent_service, message_service

router = APIRouter(prefix="/agents", tags=["agents"])


def _build_messages(system_message: BaseMessage, user_message: str) -> list[BaseMessage]:
    return [system_message, HumanMessage(content=user_message)]


def _direct_tool_signature(tool_call: dict[str, object]) -> tuple[str, str]:
    name = str(tool_call.get("name") or "")
    args = tool_call.get("args")
    args_signature = repr(sorted(args.items())) if isinstance(args, dict) else repr(args)
    return (name, args_signature)


async def _direct_agent_tool_result(
    request: Request,
    db: AsyncSession,
    current_user: User,
    caller_agent: Agent,
    requested_agent_id: str,
    task: str,
    instructions: str | None = None,
) -> str:
    _ = (request, task, instructions)
    context = await build_agent_invocation_context(db, caller_agent, current_user)
    assistant = await message_service._resolve_bound_assistant(  # noqa: SLF001
        context,
        requested_agent_id,
    )
    return json.dumps(
        {
            "tool": "AgentAsTool",
            "status": "GROUP_CONTEXT_REQUIRED",
            "agent_id": str(assistant.id),
            "display_name": assistant.name,
            "message": (
                "AgentAsTool dispatches visibly inside a group chat. "
                "Invoke this agent from a group that also includes the assistant agent."
            ),
        },
        ensure_ascii=False,
    )


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


@router.get("/external-runtimes/status", response_model=ExternalAdapterStatusResponse)
async def get_external_runtime_status() -> ExternalAdapterStatusResponse:
    statuses = [
        await detect_adapter_status(adapter)
        for adapter in ADAPTER_LABELS
    ]
    return ExternalAdapterStatusResponse(
        adapters=[
            ExternalAdapterStatusRead(**asdict(status))
            for status in statuses
        ]
    )


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
    request: Request,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> InvokeResponse:
    agent = await agent_service.get_agent(db, agent_id, current_user)
    context = await build_agent_invocation_context(db, agent, current_user)
    if agent.runtime_kind == "external_cli":
        if context.workspace is None or context.workspace.local_path is None:
            raise LLMProviderError("external CLI agent requires a local workspace")
        config = normalize_external_runtime(agent.external_runtime)
        content = await run_external_agent(
            db,
            owner_id=current_user.id,
            group_id=None,
            agent_id=agent.id,
            thread_id=None,
            config=config,
            cwd=Path(context.workspace.local_path),
            prompt=f"{context.system_prompt}\n\nUser request:\n{data.message}",
        )
        return InvokeResponse(content=content)
    chat_model = await resolve_chat_model(db, agent, streaming=False)

    async def _agent_tool_executor(
        requested_agent_id: str,
        task: str,
        instructions: str | None = None,
    ) -> str:
        return await _direct_agent_tool_result(
            request,
            db,
            current_user,
            agent,
            requested_agent_id,
            task,
            instructions,
        )

    tools = build_workspace_tools(context, agent_tool_executor=_agent_tool_executor)
    model = bind_workspace_tools(chat_model, tools)
    messages = _build_messages(context.to_system_message(), data.message)
    try:
        response = await _invoke_with_tool_loop(model, tools, messages)
    except Exception as exc:
        raise LLMProviderError(f"chat_complete failed: {exc}") from exc
    if isinstance(response, AIMessage) and isinstance(response.content, str):
        content = response.content
    else:
        content = str(getattr(response, "content", response))
    return InvokeResponse(content=content)


@router.post("/{agent_id}/invoke/stream")
async def invoke_agent_stream(
    agent_id: UUID,
    data: InvokeRequest,
    request: Request,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> EventSourceResponse:
    agent = await agent_service.get_agent(db, agent_id, current_user)
    context = await build_agent_invocation_context(db, agent, current_user)
    if agent.runtime_kind == "external_cli":
        if context.workspace is None or context.workspace.local_path is None:
            raise LLMProviderError("external CLI agent requires a local workspace")
        config = normalize_external_runtime(agent.external_runtime)
        workspace_path = Path(context.workspace.local_path)

        async def external_event_gen() -> AsyncIterator[dict[str, str]]:
            try:
                async for event in run_external_agent_stream(
                    db,
                    owner_id=current_user.id,
                    group_id=None,
                    agent_id=agent.id,
                    thread_id=None,
                    config=config,
                    cwd=workspace_path,
                    prompt=f"{context.system_prompt}\n\nUser request:\n{data.message}",
                ):
                    if event.kind == "token" and isinstance(event.data, str):
                        yield {"event": "token", "data": event.data}
                    elif event.kind == "run" and isinstance(event.data, dict):
                        payload = {**event.data, "display_name": agent.name}
                        yield {"event": "external_agent_run", "data": json.dumps(payload)}
            except Exception as exc:
                yield {"event": "error", "data": f"external agent failed: {exc}"}
            yield {"event": "done", "data": ""}

        return EventSourceResponse(external_event_gen())
    chat_model = await resolve_chat_model(db, agent, streaming=True)

    async def _agent_tool_executor(
        requested_agent_id: str,
        task: str,
        instructions: str | None = None,
    ) -> str:
        return await _direct_agent_tool_result(
            request,
            db,
            current_user,
            agent,
            requested_agent_id,
            task,
            instructions,
        )

    tools = build_workspace_tools(context, agent_tool_executor=_agent_tool_executor)

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
