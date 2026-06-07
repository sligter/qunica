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
from collections.abc import AsyncIterator, Iterator
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
from app.core.exceptions import LLMProviderError

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
class RuntimeHumanInputRequest:
    question: str
    required: bool
    choices: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class RuntimeToolEvent:
    tool_call_id: str
    tool_name: str
    status: ToolEventStatus
    args_summary: str | None = None
    result_summary: str | None = None
    input_request: RuntimeHumanInputRequest | None = None


@dataclass(frozen=True, slots=True)
class RuntimeWaitForUser:
    message: str
    input_request: RuntimeHumanInputRequest | None = None


@dataclass(frozen=True, slots=True)
class RuntimeAgentHandoff:
    message: str


MAX_TOOL_SUMMARY_LENGTH = 240
_REASONING_KEYS = (
    "reasoning_content",
    "reasoning",
    "thinking",
    "thinking_content",
)

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


def _json_payload_from_result(result: str) -> dict[str, Any] | None:
    try:
        payload = json.loads(result)
    except json.JSONDecodeError:
        return None
    return payload if isinstance(payload, dict) else None


def _human_input_request_from_result(result: str) -> RuntimeHumanInputRequest | None:
    payload = _json_payload_from_result(result)
    if payload is None:
        return None
    status = str(payload.get("status") or "").upper()
    if status not in {"WAITING_FOR_USER", "INPUT_REQUESTED"}:
        return None
    message = payload.get("message")
    if not isinstance(message, str) or not message.strip():
        return None
    prefix = "Human input requested:"
    question = message.strip()
    if question.casefold().startswith(prefix.casefold()):
        question = question[len(prefix) :].strip()
    raw_choices = payload.get("choices")
    choices = (
        tuple(
            choice.strip()
            for choice in raw_choices
            if isinstance(choice, str) and choice.strip()
        )[:8]
        if isinstance(raw_choices, list)
        else ()
    )
    return RuntimeHumanInputRequest(
        question=question or message.strip(),
        required=status == "WAITING_FOR_USER",
        choices=choices,
    )


def _human_input_request_payload(
    request: RuntimeHumanInputRequest,
) -> dict[str, str | bool | list[str]]:
    payload: dict[str, str | bool | list[str]] = {
        "question": request.question,
        "required": request.required,
    }
    if request.choices:
        payload["choices"] = list(request.choices)
    return payload


def _wait_for_user_from_result(result: str) -> RuntimeWaitForUser | None:
    payload = _json_payload_from_result(result)
    if payload is None:
        return None
    status = str(payload.get("status") or "").upper()
    if status != "WAITING_FOR_USER":
        return None
    message = payload.get("message")
    return RuntimeWaitForUser(
        str(message) if message else "Waiting for your input",
        input_request=_human_input_request_from_result(result),
    )


def _agent_handoff_from_result(
    tool_name: str | None,
    result: str,
    agent_handoff_tool_names: set[str] | None = None,
) -> RuntimeAgentHandoff | None:
    terminal_names = agent_handoff_tool_names or set()
    if tool_name not in terminal_names:
        return None
    payload = _json_payload_from_result(result)
    if payload is None:
        return None
    status = str(payload.get("status") or "").upper()
    if status != "DISPATCHED":
        return None
    message = payload.get("message")
    return RuntimeAgentHandoff(str(message) if message else "Agent handoff dispatched")


def _text_from_provider_value(value: Any) -> str:
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        return "".join(_text_from_provider_value(item) for item in value)
    if isinstance(value, dict):
        for key in ("text", "content", "thinking", "reasoning"):
            text = _text_from_provider_value(value.get(key))
            if text:
                return text
        summary = value.get("summary")
        if isinstance(summary, list):
            return "".join(_text_from_provider_value(item) for item in summary)
    return ""


def _reasoning_from_mapping(value: Any) -> str:
    if not isinstance(value, dict):
        return ""
    for key in _REASONING_KEYS:
        text = _text_from_provider_value(value.get(key))
        if text:
            return text
    return ""


def _unemitted_text(value: str, emitted: str) -> str:
    if not value:
        return ""
    if not emitted:
        return value
    if value.startswith(emitted):
        return value[len(emitted) :]
    if emitted.endswith(value) or value in emitted:
        return ""
    return value


def _iter_stream_chunk_parts(chunk: Any) -> Iterator[tuple[str, str]]:
    """Split a streamed model chunk into ordered ("reasoning"|"token", text) parts.

    Handles three provider shapes:
    - OpenAI-compatible reasoning models (e.g. DeepSeek-R1) stream the chain of
      thought in ``additional_kwargs["reasoning_content"]`` while ``content`` is
      the visible answer.
    - Anthropic Claude (extended thinking) streams ``content`` as a list of
      blocks: ``{"type": "thinking", "thinking": ...}`` and
      ``{"type": "text", "text": ...}``.
    - Plain providers stream ``content`` as a string (visible text only).
    """
    extra = getattr(chunk, "additional_kwargs", None)
    reasoning = _reasoning_from_mapping(extra)
    if reasoning:
        yield ("reasoning", reasoning)
    metadata_reasoning = _reasoning_from_mapping(getattr(chunk, "response_metadata", None))
    if metadata_reasoning and metadata_reasoning != reasoning:
        yield ("reasoning", metadata_reasoning)
    content = getattr(chunk, "content", None)
    if isinstance(content, str):
        if content:
            yield ("token", content)
        return
    if isinstance(content, list):
        for block in content:
            if isinstance(block, str):
                if block:
                    yield ("token", block)
                continue
            if not isinstance(block, dict):
                continue
            block_type = block.get("type")
            if block_type in {"thinking", "reasoning", "reasoning_content", "thinking_delta"}:
                text = _text_from_provider_value(block)
                if text:
                    yield ("reasoning", text)
            elif "text" in block or block_type in {"text", "output_text"}:
                text = _text_from_provider_value(block)
                if text:
                    yield ("token", text)


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


async def _invoke_once_stream(
    graph: _Graph,
    thread_id: str,
    chat_model: BaseChatModel,
    input_messages: list[BaseMessage],
) -> AsyncIterator[tuple[str, Any]]:
    config = _config_for(thread_id, chat_model)
    state_input: GroupState = {
        "input_messages": list(input_messages),
        "last_response": None,
    }

    final_response: AIMessage | None = None
    emitted_reasoning = ""
    async for event in graph.astream_events(
        cast(Any, state_input), config=config, version="v2"
    ):
        kind = event.get("event")
        if kind == "on_chat_model_stream":
            chunk = event["data"].get("chunk")
            if chunk is None:
                continue
            for part_kind, part_text in _iter_stream_chunk_parts(chunk):
                if part_kind == "reasoning":
                    emitted_reasoning += part_text
                yield (part_kind, part_text)
        elif kind == "on_chat_model_end":
            output = event["data"].get("output")
            if isinstance(output, AIMessage):
                final_response = output

    if final_response is None:
        raise LLMProviderError("agent runtime stream produced no final response")
    final_reasoning = _reasoning_from_mapping(
        final_response.additional_kwargs
    ) or _reasoning_from_mapping(final_response.response_metadata)
    reasoning_delta = _unemitted_text(final_reasoning, emitted_reasoning)
    if reasoning_delta:
        yield ("reasoning", reasoning_delta)
    yield ("done", final_response)


def _agent_handoff_tool_call_priority(
    tool_calls: list[Any], agent_handoff_tool_names: set[str] | None
) -> list[tuple[int, Any]]:
    indexed_calls = list(enumerate(tool_calls))
    if not agent_handoff_tool_names:
        return indexed_calls
    return sorted(
        indexed_calls,
        key=lambda item: (
            _tool_call_name(cast(dict[str, Any], item[1])) not in agent_handoff_tool_names,
            item[0],
        ),
    )


async def _execute_tool_calls(
    tool_calls: list[Any],
    tools: dict[str, Any],
    tool_event_callback: Any | None = None,
    agent_handoff_tool_names: set[str] | None = None,
) -> AsyncIterator[
    tuple[RuntimeToolEvent, ToolMessage | RuntimeWaitForUser | RuntimeAgentHandoff | None]
]:
    for index, tool_call in _agent_handoff_tool_call_priority(
        tool_calls, agent_handoff_tool_names
    ):
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
                    except Exception as exc:
                        # Tool errors must be returned to the model, not crash fan-out.
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
            input_request=(
                _human_input_request_from_result(result) if name == "AskUser" else None
            ),
        )
        if tool_event_callback is not None:
            await tool_event_callback(result_event)
        handoff = _agent_handoff_from_result(name, result, agent_handoff_tool_names)
        if handoff is not None:
            yield result_event, handoff
            return
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
    agent_handoff_tool_names: set[str] | None = None,
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
            agent_handoff_tool_names=agent_handoff_tool_names,
        ):
            if isinstance(result, RuntimeAgentHandoff):
                content = _message_text(response.content).strip()
                return AIMessage(
                    content=content,
                    additional_kwargs={"agent_handoff": True, "handoff_message": result.message},
                )
            if isinstance(result, RuntimeWaitForUser):
                content = _message_text(response.content).strip() or result.message
                additional_kwargs: dict[str, Any] = {
                    "waiting_for_user": True,
                    "waiting_message": result.message,
                }
                if result.input_request is not None:
                    additional_kwargs["human_input_request"] = _human_input_request_payload(
                        result.input_request
                    )
                return AIMessage(
                    content=content,
                    additional_kwargs=additional_kwargs,
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
    agent_handoff_tool_names: set[str] | None = None,
) -> AsyncIterator[tuple[str, Any]]:
    """Stream model tokens while preserving the provider-native tool loop."""
    tools = workspace_tools or {}
    model = bind_workspace_tools(chat_model, tools)
    messages = list(input_messages)
    repeated_call_counts: Counter[tuple[str | None, str]] = Counter()

    while True:
        final: AIMessage | None = None
        emitted_content = ""
        async for kind, payload in _invoke_once_stream(graph, thread_id, model, messages):
            if kind == "token":
                emitted_content += str(payload)
                yield ("token", payload)
            elif kind == "reasoning":
                yield ("reasoning", payload)
            else:
                final = payload
        if final is None:
            raise LLMProviderError("agent runtime stream produced no final response")

        # Surface this LLM call's usage so the UI can update the context meter
        # mid-turn: the tool loop below may run several more model calls before
        # the terminal ("done") response, each one growing the prompt.
        yield ("usage", final)

        tool_calls = list(final.tool_calls or [])
        if not tool_calls:
            content = _message_text(final.content)
            content_delta = _unemitted_text(content, emitted_content)
            if content_delta:
                yield ("token", content_delta)
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
        interim_delta = _unemitted_text(interim_content, emitted_content)
        if interim_delta:
            yield ("token", interim_delta)
        messages.append(final)
        async for tool_event, result in _execute_tool_calls(
            tool_calls=tool_calls,
            tools=tools,
            tool_event_callback=tool_event_callback,
            agent_handoff_tool_names=agent_handoff_tool_names,
        ):
            yield ("tool_event", tool_event)
            if isinstance(result, RuntimeAgentHandoff):
                handoff = AIMessage(
                    content=interim_content.strip(),
                    additional_kwargs={
                        "agent_handoff": True,
                        "handoff_message": result.message,
                    },
                )
                yield ("agent_handoff", result)
                yield ("done", handoff)
                return
            if isinstance(result, RuntimeWaitForUser):
                content = interim_content.strip() or result.message
                additional_kwargs: dict[str, Any] = {
                    "waiting_for_user": True,
                    "waiting_message": result.message,
                }
                if result.input_request is not None:
                    additional_kwargs["human_input_request"] = _human_input_request_payload(
                        result.input_request
                    )
                waiting = AIMessage(
                    content=content,
                    additional_kwargs=additional_kwargs,
                )
                yield ("waiting_for_user", result)
                yield ("done", waiting)
                return
            if result is not None:
                messages.append(result)
