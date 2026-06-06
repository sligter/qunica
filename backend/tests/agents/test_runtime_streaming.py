import json
from typing import Any, cast

import pytest
from langchain_core.messages import AIMessage, HumanMessage

from app.agents import runtime


class StreamingGraph:
    async def astream_events(
        self,
        _state_input: Any,
        config: Any,
        *,
        version: str,
    ) -> Any:
        assert config is not None
        assert version == "v2"
        for chunk in ("hel", "lo"):
            yield {
                "event": "on_chat_model_stream",
                "data": {"chunk": type("Chunk", (), {"content": chunk})()},
            }
        yield {"event": "on_chat_model_end", "data": {"output": AIMessage(content="hello")}}


def _chunk(content: Any, additional_kwargs: dict[str, Any] | None = None) -> Any:
    return type(
        "Chunk",
        (),
        {"content": content, "additional_kwargs": additional_kwargs or {}},
    )()


def test_human_input_request_from_result_preserves_choices() -> None:
    result = json.dumps(
        {
            "tool": "AskUser",
            "status": "WAITING_FOR_USER",
            "message": "Human input requested: Pick a direction.",
            "choices": ["Research first", "Draft now"],
        }
    )

    request = runtime._human_input_request_from_result(result)
    wait = runtime._wait_for_user_from_result(result)

    assert request is not None
    assert request.question == "Pick a direction."
    assert request.required is True
    assert request.choices == ("Research first", "Draft now")
    assert wait is not None
    assert wait.input_request == request


class ThinkingBlockGraph:
    """Mimics Anthropic extended-thinking streaming: content is a list of blocks."""

    async def astream_events(
        self, _state_input: Any, config: Any, *, version: str
    ) -> Any:
        assert config is not None
        assert version == "v2"
        yield {
            "event": "on_chat_model_stream",
            "data": {"chunk": _chunk([{"type": "thinking", "thinking": "let me think"}])},
        }
        yield {
            "event": "on_chat_model_stream",
            "data": {"chunk": _chunk([{"type": "text", "text": "answer"}])},
        }
        yield {"event": "on_chat_model_end", "data": {"output": AIMessage(content="answer")}}


class ReasoningContentGraph:
    """Mimics OpenAI-compatible reasoning models (e.g. DeepSeek-R1)."""

    async def astream_events(
        self, _state_input: Any, config: Any, *, version: str
    ) -> Any:
        assert config is not None
        assert version == "v2"
        yield {
            "event": "on_chat_model_stream",
            "data": {"chunk": _chunk("", {"reasoning_content": "thinking..."})},
        }
        yield {
            "event": "on_chat_model_stream",
            "data": {"chunk": _chunk("final")},
        }
        yield {"event": "on_chat_model_end", "data": {"output": AIMessage(content="final")}}


@pytest.mark.asyncio
async def test_run_with_stream_streams_model_chunks_when_tools_are_bound() -> None:
    events = [
        event
        async for event in runtime.run_with_stream(
            graph=cast(Any, StreamingGraph()),
            thread_id="thread-1",
            chat_model=cast(Any, object()),
            input_messages=[HumanMessage(content="hi")],
            workspace_tools={"Read": cast(Any, object())},
        )
    ]

    assert [payload for kind, payload in events if kind == "token"] == ["hel", "lo"]
    assert [kind for kind, _payload in events] == ["token", "token", "done"]


@pytest.mark.asyncio
async def test_run_with_stream_emits_reasoning_from_thinking_blocks() -> None:
    events = [
        event
        async for event in runtime.run_with_stream(
            graph=cast(Any, ThinkingBlockGraph()),
            thread_id="thread-1",
            chat_model=cast(Any, object()),
            input_messages=[HumanMessage(content="hi")],
            workspace_tools={"Read": cast(Any, object())},
        )
    ]

    assert [(kind, payload) for kind, payload in events if kind != "done"] == [
        ("reasoning", "let me think"),
        ("token", "answer"),
    ]
    assert events[-1][0] == "done"


@pytest.mark.asyncio
async def test_run_with_stream_emits_reasoning_from_reasoning_content() -> None:
    events = [
        event
        async for event in runtime.run_with_stream(
            graph=cast(Any, ReasoningContentGraph()),
            thread_id="thread-1",
            chat_model=cast(Any, object()),
            input_messages=[HumanMessage(content="hi")],
            workspace_tools={"Read": cast(Any, object())},
        )
    ]

    assert [(kind, payload) for kind, payload in events if kind != "done"] == [
        ("reasoning", "thinking..."),
        ("token", "final"),
    ]
    assert events[-1][0] == "done"
