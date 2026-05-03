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

from collections.abc import AsyncIterator
from typing import Any, cast

from langchain_core.language_models import BaseChatModel
from langchain_core.messages import AIMessage, BaseMessage
from langchain_core.runnables import RunnableConfig
from langgraph.checkpoint.base import BaseCheckpointSaver
from langgraph.graph import END, START, StateGraph
from langgraph.graph.state import CompiledStateGraph

from app.agents.state import GroupState
from app.core.exceptions import LLMProviderError

_CHAT_MODEL_KEY = "chat_model"

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


async def run(
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
    response = (
        final_state.get("last_response") if isinstance(final_state, dict) else None
    )
    if not isinstance(response, AIMessage):
        raise LLMProviderError("agent runtime returned no response")
    return response


async def run_with_stream(
    graph: _Graph,
    thread_id: str,
    chat_model: BaseChatModel,
    input_messages: list[BaseMessage],
) -> AsyncIterator[tuple[str, Any]]:
    """Yield ('token', str) chunks during inference, then ('done', AIMessage)."""
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
            content = getattr(chunk, "content", None)
            if isinstance(content, str) and content:
                yield ("token", content)
        elif kind == "on_chat_model_end":
            output = event["data"].get("output")
            if isinstance(output, AIMessage):
                final_response = output

    if final_response is None:
        raise LLMProviderError("agent runtime stream produced no final response")
    yield ("done", final_response)
