"""LangGraph runtime for agent execution.

- One compiled graph per process, cached on `app.state.graph`.
- Graph shape: `START → agent → END` (room to grow: tool node, approval node,
  router node — all added without changing this surface).
- The chat-model used by the node is constructed by the caller
  (`message_service` via `resolve_chat_model`) and injected through
  LangGraph's `RunnableConfig.configurable["chat_model"]`. This keeps the
  runtime stateless and the service layer in charge of provider routing.
- `thread_id` (LangGraph) maps 1:1 to our `threads.id`, so the checkpointer
  rows can be joined to our business thread row by ID.

Two public entrypoints:
- `run(graph, thread_id, chat_model, input_messages) -> AIMessage` — non-streaming.
- `run_with_stream(graph, thread_id, chat_model, input_messages)` — yields
  `("token", str)` events as the LLM emits chunks, then a final
  `("done", AIMessage)`.
"""

import json
from collections import Counter
from collections.abc import AsyncIterator
from dataclasses import dataclass
from typing import Any, Literal, cast

from langchain_core.language_models import BaseChatModel
from langchain_core.messages import AIMessage, BaseMessage, ToolMessage
from langchain_core.runnables import RunnableConfig
from langgraph.checkpoint.base import BaseCheckpointSaver
from langgraph.graph import END, START, StateGraph
from langgraph.graph.state import CompiledStateGraph

from app.agents.state import GroupState
from app.agents.workspace_tools import bind_workspace_tools, execute_workspace_tool
from app.core.exceptions import AgentChatError, LLMProviderError

_CHAT_MODEL_KEY = "chat_model"
TOOL_LOOP_REPEATED_CALL_LIMIT = 8
ToolEventStatus = Literal[
    "started",
    "completed",
    "failed",
    "unavailable",
    "setup_required",
    "workspace_required",
    "input_required",
    "approval_required",
]


@dataclass(frozen=True, slots=True)
class RuntimeToolEvent:
    tool_call_id: str
    tool_name: str
    status: ToolEventStatus
    args_summary: str | None = None
    result_summary: str | None = None


@dataclass(frozen=True, slots=True)
class RuntimeWaitForUser:
    message: str


MAX_TOOL_SUMMARY_LENGTH = 240

# StateGraph and CompiledStateGraph are highly generic in langgraph 1.x; we
# pin the state type and let the rest fall to Any to avoid leaking internal
# generic positions across our public API.
_Graph = CompiledStateGraph[GroupState, Any, GroupState, GroupState]


async def _agent_node(state: GroupState, config: RunnableConfig) -> dict[str, Any]:
    configurable = config.get("configurable") or {}
    chat_model = configurable.get(_CHAT_MODEL_KEY)
    if chat_model is None:
        raise LLMProviderError("agent_node received no chat_model in configurable")
    messages = state.get("input_messages") or []
    response = await chat_model.ainvoke(messages)
    if not isinstance(response, AIMessage):
        response = AIMessage(content=str(response.content))
    return {"last_response": response}


def build_graph() -> StateGraph[GroupState, Any, GroupState, GroupState]:
    g: StateGraph[GroupState, Any, GroupState, GroupState] = StateGraph(GroupState)
    g.add_node("agent", _agent_node)
    g.add_edge(START, "agent")
    g.add_edge("agent", END)
    return g


def compile_graph(checkpointer: BaseCheckpointSaver[Any]) -> _Graph:
    return build_graph().compile(checkpointer=checkpointer)


def _config_for(thread_id: str, chat_model: BaseChatModel) -> RunnableConfig:
    return {
        "configurable": {
            "thread_id": thread_id,
            _CHAT_MODEL_KEY: chat_model,
        }
    }


def _tool_call_name(tool_call: dict[str, Any]) -> str | None:
    name = tool_call.get("name")
    return str(name) if name is not None else None


def _tool_call_args(tool_call: dict[str, Any]) -> dict[str, Any]:
    args = tool_call.get("args")
    return args if isinstance(args, dict) else {}


def _tool_call_id(tool_call: dict[str, Any], index: int) -> str:
    raw_id = tool_call.get("id")
    return str(raw_id) if raw_id else f"tool-call-{index}"


def _summarize_mapping(values: dict[str, Any]) -> str:
    if not values:
        return "no arguments"
    parts = [f"{key}={value!r}" for key, value in sorted(values.items())]
    return _bounded_summary(", ".join(parts))


def _bounded_summary(value: str, limit: int = MAX_TOOL_SUMMARY_LENGTH) -> str:
    normalized = " ".join(value.split())
    if len(normalized) <= limit:
        return normalized
    return f"{normalized[: limit - 1]}…"


def _result_summary(name: str | None, result: str) -> str:
    try:
        payload = json.loads(result)
    except json.JSONDecodeError:
        payload = None
    if isinstance(payload, dict):
        status = str(payload.get("status") or "").upper()
        message = payload.get("message")
        if status in {
            "WAITING_FOR_USER",
            "INPUT_REQUESTED",
            "SETUP_REQUIRED",
            "WORKSPACE_REQUIRED",
            "APPROVAL_REQUIRED",
            "FAILED",
            "ERROR",
        } and isinstance(message, str):
            return _bounded_summary(message)
    if name == "Read" and not result.startswith("Tool "):
        line_count = len(result.splitlines())
        return _bounded_summary(f"Read completed; returned {line_count} numbered lines.")
    if name in {"Bash", "Fetch"} and not result.startswith("Tool "):
        return _bounded_summary(f"{name} completed; returned bounded output to the model.")
    return _bounded_summary(result)


def _result_status(name: str | None, result: str) -> ToolEventStatus:
    if name is None or "Malformed tool call" in result:
        return "failed"
    if result.startswith("Tool ") and " is unavailable in this runtime." in result:
        return "unavailable"
    if result.startswith("Tool ") and " failed: " in result:
        return "failed"
    try:
        payload = json.loads(result)
    except json.JSONDecodeError:
        return "completed"
    if isinstance(payload, dict):
        status = str(payload.get("status") or "").upper()
        if status == "WAITING_FOR_USER":
            return "input_required"
        if status == "INPUT_REQUESTED":
            return "input_required"
        if status == "SETUP_REQUIRED":
            return "setup_required"
        if status == "WORKSPACE_REQUIRED":
            return "workspace_required"
        if status == "APPROVAL_REQUIRED":
            return "approval_required"
        if status == "NOT_FOUND":
            return "unavailable"
        if status in {"FAILED", "ERROR"}:
            return "failed"
    return "completed"


def _tool_call_signature(tool_call: dict[str, Any]) -> tuple[str | None, str]:
    args = _tool_call_args(tool_call)
    try:
        args_signature = repr(sorted(args.items()))
    except TypeError:
        args_signature = repr(args)
    return (_tool_call_name(tool_call), args_signature)


def _repeated_call_guard_message(name: str | None) -> str:
    display_name = name or "unknown"
    return (
        f"Tool execution paused because the model repeatedly requested the same {display_name} "
        "tool call without making progress. Summarize the completed tool results and ask the "
        "user how to proceed."
    )


def _message_text(content: Any) -> str:
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts: list[str] = []
        for item in content:
            if isinstance(item, str):
                parts.append(item)
            elif isinstance(item, dict):
                text = item.get("text")
                if isinstance(text, str):
                    parts.append(text)
        return "".join(parts)
    return str(content) if content is not None else ""


def _wait_for_user_from_result(result: str) -> RuntimeWaitForUser | None:
    try:
        payload = json.loads(result)
    except json.JSONDecodeError:
        return None
    if not isinstance(payload, dict):
        return None
    status = str(payload.get("status") or "").upper()
    if status != "WAITING_FOR_USER":
        return None
    message = payload.get("message")
    return RuntimeWaitForUser(str(message) if message else "Waiting for your input")


async def _invoke_once(
    graph: _Graph,
    thread_id: str,
    chat_model: BaseChatModel,
    input_messages: list[BaseMessage],
) -> AIMessage:
    config = _config_for(thread_id, chat_model)
    state_input: GroupState = {
        "input_messages": list(input_messages),
        "last_response": None,
    }
    final_state = await graph.ainvoke(cast(Any, state_input), config=config)
    response = final_state.get("last_response") if isinstance(final_state, dict) else None
    if not isinstance(response, AIMessage):
        raise LLMProviderError("agent runtime returned no response")
    return response


async def _execute_tool_calls(
    tool_calls: list[Any],
    tools: dict[str, Any],
    tool_event_callback: Any | None = None,
) -> AsyncIterator[tuple[RuntimeToolEvent, ToolMessage | RuntimeWaitForUser | None]]:
    for index, tool_call in enumerate(tool_calls):
        tool_call_dict = cast(dict[str, Any], tool_call)
        name = _tool_call_name(tool_call_dict)
        args = _tool_call_args(tool_call_dict)
        call_id = _tool_call_id(tool_call_dict, index)
        display_name = name or "unknown"
        start_event = RuntimeToolEvent(
            tool_call_id=call_id,
            tool_name=display_name,
            status="started",
            args_summary=_summarize_mapping(args),
        )
        if tool_event_callback is not None:
            await tool_event_callback(start_event)
        yield start_event, None
        if name is None:
            result = f"Malformed tool call at index {index}."
        else:
            executor = tools.get(name)
            if executor is None:
                result = execute_workspace_tool(tools, name, args)
            else:
                maybe_coroutine = executor.ainvoke(args)
                if not hasattr(maybe_coroutine, "__await__"):
                    result = str(maybe_coroutine)
                else:
                    try:
                        result = str(await maybe_coroutine)
                    except AgentChatError as exc:
                        result = json.dumps(
                            {"tool": name, "status": "FAILED", "message": str(exc)},
                            ensure_ascii=False,
                        )
        status = _result_status(name, result)
        result_event = RuntimeToolEvent(
            tool_call_id=call_id,
            tool_name=display_name,
            status=status,
            result_summary=_result_summary(name, result),
        )
        if tool_event_callback is not None:
            await tool_event_callback(result_event)
        wait_for_user = _wait_for_user_from_result(result)
        if wait_for_user is not None:
            yield result_event, wait_for_user
            return
        yield result_event, ToolMessage(content=result, tool_call_id=call_id)


async def run(
    graph: _Graph,
    thread_id: str,
    chat_model: BaseChatModel,
    input_messages: list[BaseMessage],
    workspace_tools: dict[str, Any] | None = None,
    tool_event_callback: Any | None = None,
) -> AIMessage:
    tools = workspace_tools or {}
    model = bind_workspace_tools(chat_model, tools)
    messages = list(input_messages)
    repeated_call_counts: Counter[tuple[str | None, str]] = Counter()

    while True:
        response = await _invoke_once(graph, thread_id, model, messages)
        tool_calls = list(response.tool_calls or [])
        if not tool_calls:
            return response
        for tool_call in tool_calls:
            signature = _tool_call_signature(cast(dict[str, Any], tool_call))
            repeated_call_counts[signature] += 1
            if repeated_call_counts[signature] > TOOL_LOOP_REPEATED_CALL_LIMIT:
                return AIMessage(content=_repeated_call_guard_message(signature[0]))
        messages.append(response)
        async for _event, result in _execute_tool_calls(
            tool_calls=tool_calls,
            tools=tools,
            tool_event_callback=tool_event_callback,
        ):
            if isinstance(result, RuntimeWaitForUser):
                content = _message_text(response.content).strip() or result.message
                return AIMessage(
                    content=content,
                    additional_kwargs={"waiting_for_user": True, "waiting_message": result.message},
                )
            if result is not None:
                messages.append(result)


async def run_with_stream(
    graph: _Graph,
    thread_id: str,
    chat_model: BaseChatModel,
    input_messages: list[BaseMessage],
    workspace_tools: dict[str, Any] | None = None,
    tool_event_callback: Any | None = None,
) -> AsyncIterator[tuple[str, Any]]:
    """Yield live tool events and final tokens when tools are active.

    Provider-native tool calls are not token-streamed because each tool result
    must feed a follow-up model invocation. Tool start/result events are yielded
    immediately, then the final answer is emitted as a token chunk when the loop
    completes.
    """
    if workspace_tools:
        tools = workspace_tools or {}
        model = bind_workspace_tools(chat_model, tools)
        messages = list(input_messages)
        repeated_call_counts: Counter[tuple[str | None, str]] = Counter()
        while True:
            final = await _invoke_once(graph, thread_id, model, messages)
            tool_calls = list(final.tool_calls or [])
            if not tool_calls:
                content = final.content if isinstance(final.content, str) else str(final.content)
                if content:
                    yield ("token", content)
                yield ("done", final)
                return
            for tool_call in tool_calls:
                signature = _tool_call_signature(cast(dict[str, Any], tool_call))
                repeated_call_counts[signature] += 1
                if repeated_call_counts[signature] > TOOL_LOOP_REPEATED_CALL_LIMIT:
                    guarded = AIMessage(content=_repeated_call_guard_message(signature[0]))
                    yield ("token", guarded.content)
                    yield ("done", guarded)
                    return
            interim_content = _message_text(final.content)
            if interim_content:
                yield ("token", interim_content)
            messages.append(final)
            async for tool_event, result in _execute_tool_calls(
                tool_calls=tool_calls,
                tools=tools,
                tool_event_callback=tool_event_callback,
            ):
                yield ("tool_event", tool_event)
                if isinstance(result, RuntimeWaitForUser):
                    content = interim_content.strip() or result.message
                    waiting = AIMessage(
                        content=content,
                        additional_kwargs={
                            "waiting_for_user": True,
                            "waiting_message": result.message,
                        },
                    )
                    yield ("waiting_for_user", result)
                    yield ("done", waiting)
                    return
                if result is not None:
                    messages.append(result)
    config = _config_for(thread_id, chat_model)
    state_input: GroupState = {
        "input_messages": list(input_messages),
        "last_response": None,
    }

    final_response: AIMessage | None = None
    async for event in graph.astream_events(
        cast(Any, state_input), config=config, version="v2"
    ):
        kind = event.get("event")
        if kind == "on_chat_model_stream":
            chunk = event["data"].get("chunk")
            if chunk is None:
                continue
            chunk_content = getattr(chunk, "content", None)
            if isinstance(chunk_content, str) and chunk_content:
                yield ("token", chunk_content)
        elif kind == "on_chat_model_end":
            output = event["data"].get("output")
            if isinstance(output, AIMessage):
                final_response = output

    if final_response is None:
        raise LLMProviderError("agent runtime stream produced no final response")
    yield ("done", final_response)
