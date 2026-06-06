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
