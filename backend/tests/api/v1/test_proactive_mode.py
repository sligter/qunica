import json
from collections.abc import AsyncIterator, Sequence
from pathlib import Path
from typing import Any, ClassVar, cast

import httpx
from httpx import AsyncClient
from langchain_core.language_models.fake_chat_models import GenericFakeChatModel
from langchain_core.messages import AIMessage, BaseMessage, ToolMessage
from langchain_core.tools import tool
from pydantic import Field
from sqlalchemy import select


def _patch_llm_script(monkeypatch: Any, messages: Sequence[str]) -> None:
    script = iter([AIMessage(content=message) for message in messages])

    async def _resolve_factory(_db: Any, _agent: Any, *, streaming: bool = False) -> Any:
        return GenericFakeChatModel(messages=script)

    monkeypatch.setattr("app.services.message_service.resolve_chat_model", _resolve_factory)


class RecordingFakeChatModel(GenericFakeChatModel):
    shared_calls: ClassVar[list[list[BaseMessage]]] = []
    calls: list[list[BaseMessage]] = Field(default_factory=list)

    async def ainvoke(self, input: Any, config: Any = None, **kwargs: Any) -> AIMessage:
        if isinstance(input, list):
            self.calls.append(list(input))
            self.shared_calls.append(list(input))
        result = await super().ainvoke(input, config=config, **kwargs)
        assert isinstance(result, AIMessage)
        return result

    def bind_tools(self, tools: Sequence[Any], **kwargs: Any) -> "RecordingFakeChatModel":
        return self


class NoBindRecordingFakeChatModel(RecordingFakeChatModel):
    def bind_tools(self, tools: Sequence[Any], **kwargs: Any) -> "NoBindRecordingFakeChatModel":
        raise NotImplementedError


def _patch_ai_message_script(
    monkeypatch: Any,
    messages: Sequence[AIMessage],
    model_class: type[RecordingFakeChatModel] = RecordingFakeChatModel,
) -> list[list[BaseMessage]]:
    calls: list[list[BaseMessage]] = []
    RecordingFakeChatModel.shared_calls = calls
    script = iter(messages)

    async def _resolve_factory(_db: Any, _agent: Any, *, streaming: bool = False) -> Any:
        return model_class(messages=script, calls=calls)

    monkeypatch.setattr("app.services.message_service.resolve_chat_model", _resolve_factory)
    monkeypatch.setattr("app.api.v1.agents.resolve_chat_model", _resolve_factory)
    return calls


async def _setup(
    client: AsyncClient,
    auth_headers: dict[str, str],
    agent_names: Sequence[str] = ("Echo",),
    *,
    free_speech: bool = False,
    proactive_mode: bool = True,
    proactive_reply_multiplier: int = 1,
    workspace_path: Path | None = None,
) -> tuple[str, list[tuple[str, str]]]:
    workspace = await client.post(
        "/api/v1/workspaces",
        headers=auth_headers,
        json={
            "name": "Proactive repo",
            "backend_type": "local",
            "local_path": str(workspace_path or Path.cwd()),
        },
    )
    assert workspace.status_code == 201, workspace.text
    workspace_id = cast(str, workspace.json()["id"])

    agents: list[tuple[str, str]] = []
    for name in agent_names:
        response = await client.post(
            "/api/v1/agents",
            headers=auth_headers,
            json={
                "name": name,
                "system_prompt": f"You are {name}.",
                "workspace_id": workspace_id,
            },
        )
        assert response.status_code == 201, response.text
        agents.append((cast(str, response.json()["id"]), name))

    group_response = await client.post(
        "/api/v1/groups",
        headers=auth_headers,
        json={
            "name": "ProactiveGroup",
            "workspace_id": workspace_id,
            "initial_agents": [agent_id for agent_id, _name in agents],
        },
    )
    assert group_response.status_code == 201, group_response.text
    group_id = cast(str, group_response.json()["id"])

    patch = await client.patch(
        f"/api/v1/groups/{group_id}",
        headers=auth_headers,
        json={
            "free_speech": free_speech,
            "proactive_mode": proactive_mode,
            "proactive_reply_multiplier": proactive_reply_multiplier,
        },
    )
    assert patch.status_code == 200, patch.text
    assert patch.json()["proactive_reply_multiplier"] == proactive_reply_multiplier
    return group_id, agents


async def _stream_events(
    client: AsyncClient,
    auth_headers: dict[str, str],
    group_id: str,
    content: str,
) -> list[tuple[str, str]]:
    events: list[tuple[str, str]] = []
    current_event = ""
    async with client.stream(
        "POST",
        f"/api/v1/groups/{group_id}/messages/stream",
        headers=auth_headers,
        json={"content": content},
    ) as response:
        assert response.status_code == 200
        async for line in response.aiter_lines():
            if line.startswith("event:"):
                current_event = line.split(":", 1)[1].strip()
            elif line.startswith("data:"):
                events.append((current_event, line[len("data:") :].strip()))
    return events


async def _messages(
    client: AsyncClient, auth_headers: dict[str, str], group_id: str
) -> list[dict[str, Any]]:
    response = await client.get(f"/api/v1/groups/{group_id}/messages", headers=auth_headers)
    assert response.status_code == 200, response.text
    return cast(list[dict[str, Any]], response.json())


async def test_silent_marker_suppresses_message(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any
) -> None:
    _patch_llm_script(monkeypatch, ["<SILENT>"])
    group_id, _agents = await _setup(client, auth_headers)

    events = await _stream_events(client, auth_headers, group_id, "@Echo hi")

    assert "agent_message" not in [event for event, _data in events]
    assert [event for event, _data in events].count("agent_silent") == 1
    messages = await _messages(client, auth_headers, group_id)
    assert [message["sender_type"] for message in messages] == ["user"]


async def test_silent_with_whitespace_padding(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any
) -> None:
    _patch_llm_script(monkeypatch, ["  <SILENT>\n"])
    group_id, _agents = await _setup(client, auth_headers)

    events = await _stream_events(client, auth_headers, group_id, "@Echo hi")

    assert "agent_message" not in [event for event, _data in events]
    assert [event for event, _data in events].count("agent_silent") == 1
    messages = await _messages(client, auth_headers, group_id)
    assert [message["sender_type"] for message in messages] == ["user"]


async def test_silent_partial_match_persists(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any
) -> None:
    _patch_llm_script(monkeypatch, ["<SILENT> jk just kidding"])
    group_id, agents = await _setup(client, auth_headers)

    events = await _stream_events(client, auth_headers, group_id, "@Echo hi")

    assert [event for event, _data in events].count("agent_message") == 1
    messages = await _messages(client, auth_headers, group_id)
    assert messages[-1]["sender_id"] == agents[0][0]
    assert messages[-1]["content"] == "<SILENT> jk just kidding"


async def test_all_silent_emits_silence_event(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any
) -> None:
    _patch_llm_script(monkeypatch, ["<SILENT>", "<SILENT>"])
    group_id, _agents = await _setup(
        client,
        auth_headers,
        ("Echo", "Mirror"),
        free_speech=True,
        proactive_reply_multiplier=3,
    )

    events = await _stream_events(client, auth_headers, group_id, "hello group")
    event_names = [event for event, _data in events]

    assert event_names.count("agent_silent") == 2
    assert event_names.count("agent_start") == 2
    assert "silence" in event_names
    assert event_names.index("silence") < event_names.index("done")
    messages = await _messages(client, auth_headers, group_id)
    assert [message["sender_type"] for message in messages] == ["user"]


async def test_non_stream_silent_marker_suppresses_message(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any
) -> None:
    _patch_llm_script(monkeypatch, ["<SILENT>"])
    group_id, agents = await _setup(client, auth_headers)

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo hi"},
    )

    assert response.status_code == 201, response.text
    body = response.json()
    assert body["agent_replies"] == []
    assert body["silent_turns"] == [
        {"agent_id": agents[0][0], "display_name": agents[0][1]}
    ]
    assert body["all_silent"] is True
    messages = await _messages(client, auth_headers, group_id)
    assert [message["sender_type"] for message in messages] == ["user"]


async def test_proactive_off_treats_silent_as_text(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any
) -> None:
    _patch_llm_script(monkeypatch, ["<SILENT>", "<SILENT>"])
    group_id, _agents = await _setup(client, auth_headers, proactive_mode=False)

    events = await _stream_events(client, auth_headers, group_id, "@Echo hi")
    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo hi again"},
    )

    assert [event for event, _data in events].count("agent_message") == 1
    assert "agent_silent" not in [event for event, _data in events]
    assert response.status_code == 201, response.text
    assert response.json()["agent_replies"][0]["content"] == "<SILENT>"
    assert response.json()["silent_turns"] == []
    messages = await _messages(client, auth_headers, group_id)
    agent_messages = [message for message in messages if message["sender_type"] == "agent"]
    assert [message["content"] for message in agent_messages] == ["<SILENT>", "<SILENT>"]


async def test_explicit_mention_can_still_go_silent(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any
) -> None:
    _patch_llm_script(monkeypatch, ["<SILENT>"])
    group_id, _agents = await _setup(client, auth_headers)

    events = await _stream_events(client, auth_headers, group_id, "@Echo hi")
    event_names = [event for event, _data in events]

    assert "agent_message" not in event_names
    assert event_names.count("agent_silent") == 1
    assert "silence" in event_names


async def test_multi_round_continues_when_someone_spoke(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any
) -> None:
    _patch_llm_script(monkeypatch, ["hi", "<SILENT>", "<SILENT>", "now I want in"])
    group_id, agents = await _setup(
        client,
        auth_headers,
        ("Echo", "Mirror"),
        free_speech=True,
        proactive_reply_multiplier=2,
    )
    monkeypatch.setattr(
        "app.services.message_service.random.sample",
        lambda population, *, k: sorted(population, key=lambda item: item[1].name),
    )

    events = await _stream_events(client, auth_headers, group_id, "hello group")
    event_names = [event for event, _data in events]
    messages = await _messages(client, auth_headers, group_id)
    agent_messages = [message for message in messages if message["sender_type"] == "agent"]

    assert [message["sender_id"] for message in agent_messages] == [agents[0][0], agents[0][0]]
    assert [message["content"] for message in agent_messages] == ["hi", "now I want in"]
    assert event_names.count("agent_silent") == 2
    assert "silence" not in event_names


async def test_multi_round_stops_early_on_full_silence(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any
) -> None:
    _patch_llm_script(
        monkeypatch,
        ["echo 1", "mirror 1", "<SILENT>", "<SILENT>", "SHOULD NOT CALL"],
    )
    group_id, _agents = await _setup(
        client,
        auth_headers,
        ("Echo", "Mirror"),
        free_speech=True,
        proactive_reply_multiplier=3,
    )

    events = await _stream_events(client, auth_headers, group_id, "hello group")
    event_names = [event for event, _data in events]
    messages = await _messages(client, auth_headers, group_id)
    agent_messages = [message for message in messages if message["sender_type"] == "agent"]

    assert event_names.count("agent_start") == 4
    assert [message["content"] for message in agent_messages] == ["echo 1", "mirror 1"]


async def test_reply_multiplier_validation_rejects_too_low_and_has_no_upper_bound(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    group_id, _agents = await _setup(client, auth_headers)

    too_low = await client.patch(
        f"/api/v1/groups/{group_id}",
        headers=auth_headers,
        json={"proactive_reply_multiplier": 0},
    )
    valid_large = await client.patch(
        f"/api/v1/groups/{group_id}",
        headers=auth_headers,
        json={"proactive_reply_multiplier": 50},
    )

    assert too_low.status_code == 422
    assert valid_large.status_code == 200
    assert valid_large.json()["proactive_reply_multiplier"] == 50


async def test_round_order_join_then_random(
    client: AsyncClient,
    auth_headers: dict[str, str],
    monkeypatch: Any,
) -> None:
    _patch_llm_script(monkeypatch, ["A1", "B1", "C1", "A2", "C2", "B2"])
    group_id, agents = await _setup(
        client,
        auth_headers,
        ("A", "B", "C"),
        free_speech=True,
        proactive_reply_multiplier=2,
    )

    def reverse_sample(population: Sequence[Any], *, k: int) -> list[Any]:
        assert k == len(population)
        return [population[2], population[0], population[1]]

    monkeypatch.setattr("app.services.message_service.random.sample", reverse_sample)

    await _stream_events(client, auth_headers, group_id, "hello group")
    messages = await _messages(client, auth_headers, group_id)
    agent_messages = [message for message in messages if message["sender_type"] == "agent"]

    assert [message["sender_id"] for message in agent_messages] == [
        agents[0][0],
        agents[1][0],
        agents[2][0],
        agents[0][0],
        agents[2][0],
        agents[1][0],
    ]
    assert [message["content"] for message in agent_messages] == [
        "A1",
        "B1",
        "C1",
        "A2",
        "C2",
        "B2",
    ]


async def test_non_stream_rotates_previous_visible_speaker_after_other_candidate(
    client: AsyncClient,
    auth_headers: dict[str, str],
    monkeypatch: Any,
) -> None:
    _patch_llm_script(monkeypatch, ["first 1", "<SILENT>", "other 2", "first 2"])
    group_id, agents = await _setup(
        client,
        auth_headers,
        ("Echo", "Mirror"),
        free_speech=True,
        proactive_reply_multiplier=2,
    )

    def previous_speaker_first_sample(population: Sequence[Any], *, k: int) -> list[Any]:
        assert k == len(population)
        return list(population)

    monkeypatch.setattr(
        "app.services.message_service.random.sample",
        previous_speaker_first_sample,
    )

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "hello group"},
    )

    assert response.status_code == 201, response.text
    messages = await _messages(client, auth_headers, group_id)
    agent_messages = [message for message in messages if message["sender_type"] == "agent"]
    first_agent_id = agent_messages[0]["sender_id"]
    other_agent_id = agents[1][0] if first_agent_id == agents[0][0] else agents[0][0]

    assert [message["sender_id"] for message in agent_messages] == [
        first_agent_id,
        other_agent_id,
        first_agent_id,
    ]
    assert [message["content"] for message in agent_messages] == [
        "first 1",
        "other 2",
        "first 2",
    ]


async def test_visible_reply_budget_caps_non_stream_replies(
    client: AsyncClient,
    auth_headers: dict[str, str],
    monkeypatch: Any,
) -> None:
    _patch_llm_script(
        monkeypatch,
        ["E1", "M1", "E2", "M2", "E3", "M3", "SHOULD NOT CALL"],
    )
    group_id, _agents = await _setup(
        client,
        auth_headers,
        ("Echo", "Mirror"),
        free_speech=True,
        proactive_reply_multiplier=3,
    )
    monkeypatch.setattr(
        "app.services.message_service.random.sample",
        lambda population, *, k: sorted(population, key=lambda item: item[1].name),
    )

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "hello group"},
    )

    assert response.status_code == 201, response.text
    messages = await _messages(client, auth_headers, group_id)
    agent_messages = [message for message in messages if message["sender_type"] == "agent"]
    assert [message["content"] for message in agent_messages] == [
        "E1",
        "M1",
        "E2",
        "M2",
        "E3",
        "M3",
    ]


async def test_silent_turns_do_not_consume_visible_reply_budget(
    client: AsyncClient,
    auth_headers: dict[str, str],
    monkeypatch: Any,
) -> None:
    _patch_llm_script(monkeypatch, ["E1", "<SILENT>", "M2", "E2", "M3", "SHOULD NOT CALL"])
    group_id, _agents = await _setup(
        client,
        auth_headers,
        ("Echo", "Mirror"),
        free_speech=True,
        proactive_reply_multiplier=2,
    )
    calls = 0

    def rotating_sample(population: Sequence[Any], *, k: int) -> list[Any]:
        nonlocal calls
        calls += 1
        assert k == len(population)
        if calls == 1:
            return list(population)
        return [population[1], population[0]]

    monkeypatch.setattr("app.services.message_service.random.sample", rotating_sample)

    events = await _stream_events(client, auth_headers, group_id, "hello group")
    event_names = [event for event, _data in events]
    messages = await _messages(client, auth_headers, group_id)
    agent_messages = [message for message in messages if message["sender_type"] == "agent"]

    assert event_names.count("agent_silent") == 1
    assert event_names.count("agent_message") == 4
    assert [message["content"] for message in agent_messages] == ["E1", "M2", "E2", "M3"]


async def test_agent_visible_content_strips_reasoning_and_pseudo_tool_markup(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any
) -> None:
    _patch_llm_script(
        monkeypatch,
        ["<think>private reasoning</think>Before <tool_call>{}</tool_call> after"],
    )
    group_id, _agents = await _setup(client, auth_headers)

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo hi"},
    )

    assert response.status_code == 201, response.text
    content = response.json()["agent_replies"][0]["content"]
    assert "private reasoning" not in content
    assert "<think" not in content
    assert "<tool_call" not in content
    assert "Non-executed tool markup removed" in content


async def test_native_glob_tool_call_executes_and_loops_to_final_message(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any, tmp_path: Path
) -> None:
    (tmp_path / "deck.md").write_text("slides", encoding="utf-8")
    (tmp_path / "brand.txt").write_text("brand", encoding="utf-8")
    calls = _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="",
                tool_calls=[{"name": "Glob", "args": {"pattern": "*"}, "id": "glob-1"}],
            ),
            AIMessage(content="I found brand.txt and deck.md."),
        ],
    )
    group_id, _agents = await _setup(client, auth_headers, workspace_path=tmp_path)

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo list files"},
    )

    assert response.status_code == 201, response.text
    content = response.json()["agent_replies"][0]["content"]
    assert content == "I found brand.txt and deck.md."
    assert "Non-executed tool markup removed" not in content
    assert len(calls) == 2
    assert any(
        isinstance(message, ToolMessage) and "brand.txt" in str(message.content)
        for message in calls[1]
    )


async def test_tool_loop_degrades_safely_when_model_bind_tools_fails(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any, tmp_path: Path
) -> None:
    (tmp_path / "deck.md").write_text("slides", encoding="utf-8")
    calls = _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="",
                tool_calls=[{"name": "Glob", "args": {"pattern": "*"}, "id": "glob-1"}],
            ),
            AIMessage(content="I recovered after tool binding failed."),
        ],
        model_class=NoBindRecordingFakeChatModel,
    )
    group_id, _agents = await _setup(client, auth_headers, workspace_path=tmp_path)

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo list files"},
    )

    assert response.status_code == 201, response.text
    assert response.json()["agent_replies"][0]["content"] == (
        "I recovered after tool binding failed."
    )
    assert len(calls) == 2
    assert any(
        isinstance(message, ToolMessage) and "deck.md" in str(message.content)
        for message in calls[1]
    )


async def test_tool_loop_allows_more_than_five_sequential_tool_calls(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any, tmp_path: Path
) -> None:
    for index in range(6):
        (tmp_path / f"file-{index}.txt").write_text(str(index), encoding="utf-8")
    calls = _patch_ai_message_script(
        monkeypatch,
        [
            *[
                AIMessage(
                    content="",
                    tool_calls=[
                        {
                            "name": "Glob",
                            "args": {"pattern": f"file-{index}.txt"},
                            "id": f"glob-{index}",
                        }
                    ],
                )
                for index in range(6)
            ],
            AIMessage(content="Finished after six tool calls."),
        ],
    )
    group_id, _agents = await _setup(client, auth_headers, workspace_path=tmp_path)

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo inspect files"},
    )

    assert response.status_code == 201, response.text
    assert response.json()["agent_replies"][0]["content"] == "Finished after six tool calls."
    assert len(calls) == 7
    assert sum(isinstance(message, ToolMessage) for message in calls[-1]) == 6


async def test_ask_user_tool_call_sets_non_stream_waiting_for_user(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any, tmp_path: Path
) -> None:
    calls = _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="I need your input before continuing.",
                tool_calls=[
                    {
                        "name": "AskUser",
                        "args": {"question": "Please upload the draft.", "required": True},
                        "id": "ask-1",
                    }
                ],
            ),
            AIMessage(content="SHOULD NOT CALL"),
        ],
    )
    group_id, agents = await _setup(
        client,
        auth_headers,
        ("Echo", "Mirror"),
        free_speech=True,
        proactive_reply_multiplier=3,
        workspace_path=tmp_path,
    )
    patch = await client.patch(
        f"/api/v1/agents/{agents[0][0]}",
        headers=auth_headers,
        json={"tool_config": {"tools": {"ask_user": {"enabled": True}}}},
    )
    assert patch.status_code == 200, patch.text

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "hello group"},
    )

    assert response.status_code == 201, response.text
    body = response.json()
    assert body["waiting_for_user"] is True
    assert body["warnings"] == ["Human input requested: Please upload the draft."]
    assert [reply["content"] for reply in body["agent_replies"]] == [
        "I need your input before continuing."
    ]
    assert len(calls) == 1


async def test_stream_ask_user_emits_intermediate_text_tool_result_and_waiting(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any, tmp_path: Path
) -> None:
    _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="I need your input before continuing.",
                tool_calls=[
                    {
                        "name": "AskUser",
                        "args": {"question": "Please paste the missing outline.", "required": True},
                        "id": "ask-stream-1",
                    }
                ],
            ),
            AIMessage(content="SHOULD NOT CALL"),
        ],
    )
    group_id, agents = await _setup(
        client,
        auth_headers,
        ("Echo", "Mirror"),
        free_speech=True,
        proactive_reply_multiplier=3,
        workspace_path=tmp_path,
    )
    patch = await client.patch(
        f"/api/v1/agents/{agents[0][0]}",
        headers=auth_headers,
        json={"tool_config": {"tools": {"ask_user": {"enabled": True}}}},
    )
    assert patch.status_code == 200, patch.text

    events = await _stream_events(client, auth_headers, group_id, "hello group")
    event_names = [event for event, _data in events]
    tokens = [json.loads(data)["delta"] for event, data in events if event == "token"]
    result_payloads = [json.loads(data) for event, data in events if event == "tool_call_result"]

    assert "I need your input before continuing." in "".join(tokens)
    assert event_names.index("token") < event_names.index("tool_call_start")
    assert event_names.index("tool_call_result") < event_names.index("agent_message")
    assert event_names.index("agent_message") < event_names.index("waiting_for_user")
    assert event_names.index("waiting_for_user") < event_names.index("done")
    assert result_payloads[0]["tool_name"] == "AskUser"
    assert result_payloads[0]["status"] == "input_required"
    assert (
        result_payloads[0]["result_summary"]
        == "Human input requested: Please paste the missing outline."
    )
    assert event_names.count("agent_message") == 1
    assert event_names.count("agent_start") == 1


async def test_stream_native_tool_loop_emits_intermediate_text_before_tool_events(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any, tmp_path: Path
) -> None:
    (tmp_path / "notes.txt").write_text("notes", encoding="utf-8")
    _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="I'll inspect the workspace first. ",
                tool_calls=[{"name": "Glob", "args": {"pattern": "*"}, "id": "glob-text-1"}],
            ),
            AIMessage(content="The workspace contains notes.txt."),
        ],
    )
    group_id, _agents = await _setup(client, auth_headers, workspace_path=tmp_path)

    events = await _stream_events(client, auth_headers, group_id, "@Echo list files")
    event_names = [event for event, _data in events]
    tokens = [json.loads(data)["delta"] for event, data in events if event == "token"]

    assert event_names.index("token") < event_names.index("tool_call_start")
    assert "I'll inspect the workspace first. " in "".join(tokens)
    assert "The workspace contains notes.txt." in "".join(tokens)


async def test_stream_native_tool_loop_emits_live_tool_events_before_final_message(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any, tmp_path: Path
) -> None:
    (tmp_path / "notes.txt").write_text("notes", encoding="utf-8")
    _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="",
                tool_calls=[{"name": "Glob", "args": {"pattern": "*"}, "id": "glob-1"}],
            ),
            AIMessage(content="The workspace contains notes.txt."),
        ],
    )
    group_id, _agents = await _setup(client, auth_headers, workspace_path=tmp_path)

    events = await _stream_events(client, auth_headers, group_id, "@Echo list files")
    tokens = [json.loads(data)["delta"] for event, data in events if event == "token"]
    messages = await _messages(client, auth_headers, group_id)
    agent_messages = [message for message in messages if message["sender_type"] == "agent"]

    event_names = [event for event, _data in events]
    start_payloads = [json.loads(data) for event, data in events if event == "tool_call_start"]
    result_payloads = [json.loads(data) for event, data in events if event == "tool_call_result"]

    assert event_names.index("tool_call_start") < event_names.index("tool_call_result")
    assert event_names.index("tool_call_result") < event_names.index("agent_message")
    assert event_names.index("agent_message") < event_names.index("done")
    assert start_payloads[0]["tool_name"] == "Glob"
    assert start_payloads[0]["status"] == "started"
    assert result_payloads[0]["tool_name"] == "Glob"
    assert result_payloads[0]["status"] == "completed"
    assert len(result_payloads[0]["result_summary"]) <= 240
    assert "notes.txt" in result_payloads[0]["result_summary"]
    assert "The workspace contains notes.txt." in "".join(tokens)
    assert agent_messages[0]["content"] == "The workspace contains notes.txt."
    assert "Non-executed tool markup removed" not in agent_messages[0]["content"]


async def test_read_tool_result_event_does_not_leak_file_contents(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any, tmp_path: Path
) -> None:
    (tmp_path / "secret.txt").write_text("top secret brand plan", encoding="utf-8")
    _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="",
                tool_calls=[{"name": "Read", "args": {"file_path": "secret.txt"}, "id": "read-1"}],
            ),
            AIMessage(content="I read the requested file."),
        ],
    )
    group_id, _agents = await _setup(client, auth_headers, workspace_path=tmp_path)

    events = await _stream_events(client, auth_headers, group_id, "@Echo read secret.txt")

    result_payloads = [json.loads(data) for event, data in events if event == "tool_call_result"]
    assert result_payloads[0]["tool_name"] == "Read"
    assert result_payloads[0]["status"] == "completed"
    assert result_payloads[0]["result_summary"] == "Read completed; returned 1 numbered lines."
    assert "top secret brand plan" not in result_payloads[0]["result_summary"]


async def test_selected_write_tool_executes_under_workspace_root(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any, tmp_path: Path
) -> None:
    calls = _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="",
                tool_calls=[
                    {
                        "name": "Write",
                        "args": {"file_path": "nested/deck.txt", "content": "slides"},
                        "id": "write-1",
                    }
                ],
            ),
            AIMessage(content="I wrote the deck file."),
        ],
    )
    group_id, agents = await _setup(client, auth_headers, workspace_path=tmp_path)
    agent_id = agents[0][0]
    response = await client.patch(
        f"/api/v1/agents/{agent_id}",
        headers=auth_headers,
        json={
            "tool_config": {
                "tools": {
                    "read": {"enabled": False},
                    "glob": {"enabled": False},
                    "grep": {"enabled": False},
                    "write": {"enabled": True},
                }
            }
        },
    )
    assert response.status_code == 200, response.text

    events = await _stream_events(client, auth_headers, group_id, "@Echo write a file")

    assert "tool_call_start" in [event for event, _data in events]
    assert "tool_call_result" in [event for event, _data in events]
    assert len(calls) == 2
    assert (tmp_path / "nested" / "deck.txt").read_text(encoding="utf-8") == "slides"


async def test_write_tool_rejects_traversal_and_returns_error_to_model(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any, tmp_path: Path
) -> None:
    calls = _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="",
                tool_calls=[
                    {
                        "name": "Write",
                        "args": {"file_path": "../deck.txt", "content": "slides"},
                        "id": "write-1",
                    }
                ],
            ),
            AIMessage(content="I cannot write outside the workspace."),
        ],
    )
    group_id, agents = await _setup(client, auth_headers, workspace_path=tmp_path)
    agent_id = agents[0][0]
    response = await client.patch(
        f"/api/v1/agents/{agent_id}",
        headers=auth_headers,
        json={"tool_config": {"tools": {"write": {"enabled": True}}}},
    )
    assert response.status_code == 200, response.text

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo write outside"},
    )

    assert response.status_code == 201, response.text
    assert len(calls) == 2
    assert any(
        isinstance(message, ToolMessage)
        and "stay inside the workspace root" in str(message.content)
        for message in calls[1]
    )
    assert not (tmp_path.parent / "deck.txt").exists()


async def test_workspace_tool_rejects_traversal_and_returns_error_to_model(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any, tmp_path: Path
) -> None:
    calls = _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="",
                tool_calls=[
                    {"name": "Read", "args": {"file_path": "../secret.txt"}, "id": "read-1"}
                ],
            ),
            AIMessage(content="I cannot read outside the workspace."),
        ],
    )
    group_id, _agents = await _setup(client, auth_headers, workspace_path=tmp_path)

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo read outside"},
    )

    assert response.status_code == 201, response.text
    assert len(calls) == 2
    assert any(
        isinstance(message, ToolMessage)
        and "stay inside the workspace root" in str(message.content)
        for message in calls[1]
    )


async def test_edit_tool_replaces_exact_text_and_requires_replace_all_for_duplicates(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any, tmp_path: Path
) -> None:
    target = tmp_path / "notes.txt"
    target.write_text("alpha beta beta", encoding="utf-8")
    calls = _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="",
                tool_calls=[
                    {
                        "name": "Edit",
                        "args": {
                            "file_path": "notes.txt",
                            "old_string": "alpha",
                            "new_string": "ALPHA",
                        },
                        "id": "edit-1",
                    }
                ],
            ),
            AIMessage(
                content="",
                tool_calls=[
                    {
                        "name": "Edit",
                        "args": {
                            "file_path": "notes.txt",
                            "old_string": "beta",
                            "new_string": "BETA",
                        },
                        "id": "edit-2",
                    }
                ],
            ),
            AIMessage(
                content="",
                tool_calls=[
                    {
                        "name": "Edit",
                        "args": {
                            "file_path": "notes.txt",
                            "old_string": "beta",
                            "new_string": "BETA",
                            "replace_all": True,
                        },
                        "id": "edit-3",
                    }
                ],
            ),
            AIMessage(content="Edits complete."),
        ],
    )
    group_id, agents = await _setup(client, auth_headers, workspace_path=tmp_path)
    agent_id = agents[0][0]
    response = await client.patch(
        f"/api/v1/agents/{agent_id}",
        headers=auth_headers,
        json={"tool_config": {"tools": {"edit": {"enabled": True}}}},
    )
    assert response.status_code == 200, response.text

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo edit notes"},
    )

    assert response.status_code == 201, response.text
    assert len(calls) == 4
    assert target.read_text(encoding="utf-8") == "ALPHA BETA BETA"
    assert any(
        isinstance(message, ToolMessage) and "old_string is not unique" in str(message.content)
        for message in calls[2]
    )


async def test_bash_tool_executes_in_workspace_and_rejects_destructive_command(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any, tmp_path: Path
) -> None:
    calls = _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="",
                tool_calls=[
                    {
                        "name": "Bash",
                        "args": {"command": "python -c \"import os; print(os.getcwd())\""},
                        "id": "bash-1",
                    }
                ],
            ),
            AIMessage(
                content="",
                tool_calls=[{"name": "Bash", "args": {"command": "rm -rf ."}, "id": "bash-2"}],
            ),
            AIMessage(content="Bash checks complete."),
        ],
    )
    group_id, agents = await _setup(client, auth_headers, workspace_path=tmp_path)
    agent_id = agents[0][0]
    response = await client.patch(
        f"/api/v1/agents/{agent_id}",
        headers=auth_headers,
        json={"tool_config": {"tools": {"bash": {"enabled": True}}}},
    )
    assert response.status_code == 200, response.text

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo run bash"},
    )

    assert response.status_code == 201, response.text
    assert len(calls) == 3
    assert any(
        isinstance(message, ToolMessage) and str(tmp_path.resolve()) in str(message.content)
        for message in calls[1]
    )
    assert any(
        isinstance(message, ToolMessage)
        and "blocked by workspace safety policy" in str(message.content)
        for message in calls[2]
    )


async def test_bash_tool_blocks_wrapped_destructive_command(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any, tmp_path: Path
) -> None:
    calls = _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="",
                tool_calls=[
                    {
                        "name": "Bash",
                        "args": {"command": "command rm -rf ."},
                        "id": "bash-1",
                    }
                ],
            ),
            AIMessage(content="The wrapped destructive command was blocked."),
        ],
    )
    group_id, agents = await _setup(client, auth_headers, workspace_path=tmp_path)
    agent_id = agents[0][0]
    response = await client.patch(
        f"/api/v1/agents/{agent_id}",
        headers=auth_headers,
        json={"tool_config": {"tools": {"bash": {"enabled": True}}}},
    )
    assert response.status_code == 200, response.text

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo run wrapped bash"},
    )

    assert response.status_code == 201, response.text
    assert len(calls) == 2
    assert any(
        isinstance(message, ToolMessage)
        and "blocked by workspace safety policy" in str(message.content)
        for message in calls[1]
    )


async def test_fetch_tool_fetches_bounded_text_response(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any, tmp_path: Path
) -> None:
    calls = _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="",
                tool_calls=[
                    {"name": "Fetch", "args": {"url": "https://example.test/page"}, "id": "fetch-1"}
                ],
            ),
            AIMessage(content="Fetched the page."),
        ],
    )

    def _handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(200, text="hello from the web", request=request)

    class MockClient(httpx.Client):
        def __init__(self, *args: Any, **kwargs: Any) -> None:
            super().__init__(transport=httpx.MockTransport(_handler), follow_redirects=True)

    monkeypatch.setattr(httpx, "Client", MockClient)
    group_id, agents = await _setup(client, auth_headers, workspace_path=tmp_path)
    agent_id = agents[0][0]
    response = await client.patch(
        f"/api/v1/agents/{agent_id}",
        headers=auth_headers,
        json={"tool_config": {"tools": {"fetch": {"enabled": True}}}},
    )
    assert response.status_code == 200, response.text

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo fetch URL"},
    )

    assert response.status_code == 201, response.text
    assert len(calls) == 2
    assert any(
        isinstance(message, ToolMessage) and "hello from the web" in str(message.content)
        for message in calls[1]
    )


async def test_web_search_catalog_is_marked_available(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    response = await client.get("/api/v1/agents/tool-catalog", headers=auth_headers)

    assert response.status_code == 200, response.text
    web_search = next(tool for tool in response.json()["tools"] if tool["id"] == "web_search")
    assert web_search["runtime_status"] == "available"


async def test_workspace_tool_rejects_windows_absolute_paths_on_posix(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any, tmp_path: Path
) -> None:
    calls = _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="",
                tool_calls=[
                    {"name": "Read", "args": {"file_path": "C:/secret.txt"}, "id": "read-1"}
                ],
            ),
            AIMessage(content="That absolute path is outside the workspace."),
        ],
    )
    group_id, _agents = await _setup(client, auth_headers, workspace_path=tmp_path)

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo read outside"},
    )

    assert response.status_code == 201, response.text
    assert len(calls) == 2
    assert any(
        isinstance(message, ToolMessage)
        and "path must be relative to the workspace root" in str(message.content)
        for message in calls[1]
    )


async def test_group_workspace_sharing_uses_group_workspace_for_tools(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any, tmp_path: Path
) -> None:
    agent_root = tmp_path / "agent"
    group_root = tmp_path / "group"
    agent_root.mkdir()
    group_root.mkdir()
    (agent_root / "agent-only.txt").write_text("agent", encoding="utf-8")
    (group_root / "group-only.txt").write_text("group", encoding="utf-8")
    calls = _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="",
                tool_calls=[{"name": "Glob", "args": {"pattern": "*"}, "id": "glob-1"}],
            ),
            AIMessage(content="I see group-only.txt."),
        ],
    )

    agent_workspace = await client.post(
        "/api/v1/workspaces",
        headers=auth_headers,
        json={"name": "Agent ws", "backend_type": "local", "local_path": str(agent_root)},
    )
    assert agent_workspace.status_code == 201, agent_workspace.text
    group_workspace = await client.post(
        "/api/v1/workspaces",
        headers=auth_headers,
        json={"name": "Group ws", "backend_type": "local", "local_path": str(group_root)},
    )
    assert group_workspace.status_code == 201, group_workspace.text
    agent_response = await client.post(
        "/api/v1/agents",
        headers=auth_headers,
        json={
            "name": "Echo",
            "system_prompt": "You are Echo.",
            "workspace_id": agent_workspace.json()["id"],
        },
    )
    assert agent_response.status_code == 201, agent_response.text
    group_response = await client.post(
        "/api/v1/groups",
        headers=auth_headers,
        json={
            "name": "SharedGroup",
            "workspace_id": group_workspace.json()["id"],
            "initial_agents": [agent_response.json()["id"]],
        },
    )
    assert group_response.status_code == 201, group_response.text
    group_id = cast(str, group_response.json()["id"])
    toggle = await client.patch(
        f"/api/v1/groups/{group_id}/agents/{agent_response.json()['id']}/workspace-sharing",
        headers=auth_headers,
        json={"share_group_workspace": True},
    )
    assert toggle.status_code == 200, toggle.text

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo list files"},
    )

    assert response.status_code == 201, response.text
    assert any(
        isinstance(message, ToolMessage)
        and "group-only.txt" in str(message.content)
        and "agent-only.txt" not in str(message.content)
        for message in calls[1]
    )


async def test_agent_as_tool_non_stream_persists_visible_dispatch_and_helper_identity(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any, tmp_path: Path
) -> None:
    async def _fake_run(**kwargs: Any) -> AIMessage:
        workspace_tools = kwargs["workspace_tools"]
        input_messages = kwargs["input_messages"]
        if "AgentAsTool" in workspace_tools:
            result = await workspace_tools["AgentAsTool"].ainvoke(
                {"agent_id": "Coder", "task": "整理工作目录", "instructions": "keep it brief"}
            )
            assert json.loads(result)["status"] == "DISPATCHED"
            return AIMessage(content="", additional_kwargs={"agent_handoff": True})
        assert any("@Coder 整理工作目录" in str(message.content) for message in input_messages)
        return AIMessage(content="Tony completed the directory summary.")

    monkeypatch.setattr("app.services.message_service.runtime.run", _fake_run)
    group_id, agents = await _setup(
        client,
        auth_headers,
        ("Mike", "Coder"),
        proactive_mode=False,
        workspace_path=tmp_path,
    )
    mike_id, coder_id = agents[0][0], agents[1][0]
    patch = await client.patch(
        f"/api/v1/agents/{mike_id}",
        headers=auth_headers,
        json={"tool_config": {"assistant_agents": [{"agent_id": coder_id, "enabled": True}]}},
    )
    assert patch.status_code == 200, patch.text

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Mike 调用你的助手整理下工作目录"},
    )

    assert response.status_code == 201, response.text
    body = response.json()
    assert [message["sender_id"] for message in body["dispatch_messages"]] == [mike_id]
    assert body["dispatch_messages"][0]["content"].startswith("@Coder 整理工作目录")
    assert [message["sender_id"] for message in body["agent_replies"]] == [coder_id]
    assert body["agent_replies"][-1]["content"] == "Tony completed the directory summary."
    history = await _messages(client, auth_headers, group_id)
    agent_history = [message for message in history if message["sender_type"] == "agent"]
    assert [message["sender_id"] for message in agent_history] == [mike_id, coder_id]
    assert agent_history[0]["content"].startswith("@Coder 整理工作目录")


async def test_agent_as_tool_stream_emits_dispatch_before_helper_reply_identity(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any, tmp_path: Path
) -> None:
    async def _fake_run_with_stream(**kwargs: Any) -> AsyncIterator[tuple[str, Any]]:
        workspace_tools = kwargs["workspace_tools"]
        input_messages = kwargs["input_messages"]
        if "AgentAsTool" in workspace_tools:
            result = await workspace_tools["AgentAsTool"].ainvoke(
                {"agent_id": "Coder", "task": "整理工作目录"}
            )
            assert json.loads(result)["status"] == "DISPATCHED"
            yield ("agent_handoff", object())
            yield ("done", AIMessage(content="", additional_kwargs={"agent_handoff": True}))
            return
        assert any("@Coder 整理工作目录" in str(message.content) for message in input_messages)
        yield ("token", "Tony completed the directory summary.")
        yield ("done", AIMessage(content="Tony completed the directory summary."))

    monkeypatch.setattr(
        "app.services.message_service.runtime.run_with_stream", _fake_run_with_stream
    )
    group_id, agents = await _setup(
        client,
        auth_headers,
        ("Mike", "Coder"),
        proactive_mode=False,
        workspace_path=tmp_path,
    )
    mike_id, coder_id = agents[0][0], agents[1][0]
    patch = await client.patch(
        f"/api/v1/agents/{mike_id}",
        headers=auth_headers,
        json={"tool_config": {"assistant_agents": [{"agent_id": coder_id, "enabled": True}]}},
    )
    assert patch.status_code == 200, patch.text

    events = await _stream_events(
        client, auth_headers, group_id, "@Mike 调用你的助手整理下工作目录"
    )
    agent_messages = [
        json.loads(data) for event, data in events if event == "agent_message"
    ]

    assert [message["sender_id"] for message in agent_messages] == [mike_id, coder_id]
    assert agent_messages[0]["content"].startswith("@Coder 整理工作目录")
    assert agent_messages[1]["content"] == "Tony completed the directory summary."
    history = await _messages(client, auth_headers, group_id)
    agent_history = [message for message in history if message["sender_type"] == "agent"]
    assert [message["sender_id"] for message in agent_history] == [mike_id, coder_id]


async def test_agent_as_tool_resolves_helper_by_group_display_name(
    client: AsyncClient,
    auth_headers: dict[str, str],
    monkeypatch: Any,
    db_session: Any,
    tmp_path: Path,
) -> None:
    async def _fake_run(**kwargs: Any) -> AIMessage:
        workspace_tools = kwargs["workspace_tools"]
        if "AgentAsTool" in workspace_tools:
            await workspace_tools["AgentAsTool"].ainvoke(
                {"agent_id": "Coder", "task": "整理工作目录"}
            )
            return AIMessage(content="delegated")
        return AIMessage(content="helper reply")

    monkeypatch.setattr("app.services.message_service.runtime.run", _fake_run)
    group_id, agents = await _setup(
        client,
        auth_headers,
        ("Mike", "Coder"),
        proactive_mode=False,
        workspace_path=tmp_path,
    )
    mike_id, coder_id = agents[0][0], agents[1][0]
    patch = await client.patch(
        f"/api/v1/agents/{mike_id}",
        headers=auth_headers,
        json={"tool_config": {"assistant_agents": [{"agent_id": coder_id, "enabled": True}]}},
    )
    assert patch.status_code == 200, patch.text
    from app.models.group_agent import GroupAgent

    group_agent = await db_session.scalar(
        select(GroupAgent).where(
            GroupAgent.group_id == group_id,
            GroupAgent.agent_id == coder_id,
        )
    )
    assert group_agent is not None
    group_agent.display_name = "Tony"
    await db_session.flush()

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Mike dispatch"},
    )

    assert response.status_code == 201, response.text
    assert response.json()["dispatch_messages"][0]["content"].startswith("@Tony 整理工作目录")
    assert response.json()["agent_replies"][-1]["sender_id"] == coder_id


async def test_stream_sanitization_resumes_after_internal_blocks(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any
) -> None:
    _patch_llm_script(
        monkeypatch,
        ["Before <think>private reasoning</think> middle <tool_call>{}</tool_call> after"],
    )
    group_id, _agents = await _setup(client, auth_headers)

    events = await _stream_events(client, auth_headers, group_id, "@Echo hi")
    tokens = [data for event, data in events if event == "token"]
    messages = await _messages(client, auth_headers, group_id)
    agent_messages = [message for message in messages if message["sender_type"] == "agent"]
    content = agent_messages[0]["content"]

    assert "private reasoning" not in "".join(tokens)
    assert "<think" not in "".join(tokens)
    assert "<tool_call" not in "".join(tokens)
    assert "after" in "".join(tokens)
    assert content == (
        "Before  middle "
        "[Non-executed tool markup removed: this runtime did not execute a tool call.]"
        " after"
    )


async def test_proactive_stream_waiting_for_user_stops_fanout(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any
) -> None:
    me = await client.get("/api/v1/auth/me", headers=auth_headers)
    sender_name = cast(str, me.json()["name"])
    _patch_llm_script(monkeypatch, [f"@{sender_name} please provide the draft", "SHOULD NOT CALL"])
    group_id, _agents = await _setup(
        client,
        auth_headers,
        ("Echo", "Mirror"),
        free_speech=True,
        proactive_reply_multiplier=3,
    )

    events = await _stream_events(client, auth_headers, group_id, "hello group")
    event_names = [event for event, _data in events]
    messages = await _messages(client, auth_headers, group_id)
    agent_messages = [message for message in messages if message["sender_type"] == "agent"]

    assert event_names.count("agent_message") == 1
    assert "waiting_for_user" in event_names
    assert event_names.index("waiting_for_user") < event_names.index("done")
    assert [message["content"] for message in agent_messages] == [
        f"@{sender_name} please provide the draft"
    ]


async def test_non_stream_waiting_for_user_stops_fanout(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any
) -> None:
    me = await client.get("/api/v1/auth/me", headers=auth_headers)
    sender_name = cast(str, me.json()["name"])
    _patch_llm_script(
        monkeypatch,
        [f"{sender_name}, please upload the missing content", "SHOULD NOT CALL"],
    )
    group_id, _agents = await _setup(
        client,
        auth_headers,
        ("Echo", "Mirror"),
        free_speech=True,
        proactive_reply_multiplier=3,
    )

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "hello group"},
    )

    assert response.status_code == 201, response.text
    body = response.json()
    assert body["waiting_for_user"] is True
    assert body["warnings"] == ["Waiting for your input"]
    assert len(body["agent_replies"]) == 1
    messages = await _messages(client, auth_headers, group_id)
    agent_messages = [message for message in messages if message["sender_type"] == "agent"]
    assert [message["content"] for message in agent_messages] == [
        f"{sender_name}, please upload the missing content"
    ]


async def test_agent_mentions_do_not_trigger_waiting_for_user(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any
) -> None:
    _patch_llm_script(monkeypatch, ["@Mirror can you review this?", "mirror replies"])
    group_id, _agents = await _setup(
        client,
        auth_headers,
        ("Echo", "Mirror"),
        free_speech=True,
        proactive_reply_multiplier=1,
    )

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "hello group"},
    )

    assert response.status_code == 201, response.text
    body = response.json()
    assert body["waiting_for_user"] is False
    assert len(body["agent_replies"]) == 2


async def test_agent_as_tool_group_dispatch_is_visible_and_helper_routes_normally(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any
) -> None:
    group_id, agents = await _setup(
        client,
        auth_headers,
        ("Caller", "Helper"),
        free_speech=False,
        proactive_mode=False,
    )
    caller_id, helper_id = agents[0][0], agents[1][0]
    patch = await client.patch(
        f"/api/v1/agents/{caller_id}",
        headers=auth_headers,
        json={"tool_config": {"assistant_agents": [{"agent_id": helper_id, "enabled": True}]}},
    )
    assert patch.status_code == 200, patch.text
    _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="",
                tool_calls=[
                    {
                        "name": "AgentAsTool",
                        "args": {
                            "agent_id": helper_id,
                            "task": "prepare slide outline",
                            "instructions": "use the shared brief",
                        },
                        "id": "agent-tool-1",
                    }
                ],
            ),
            AIMessage(content="Helper visible answer"),
        ],
    )

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Caller delegate this"},
    )

    assert response.status_code == 201, response.text
    body = response.json()
    assert body["dispatch_messages"][0]["content"] == (
        "@Helper prepare slide outline\n\nInstructions from @Caller: use the shared brief"
    )
    assert [reply["content"] for reply in body["agent_replies"]] == [
        "Helper visible answer",
    ]
    messages = await _messages(client, auth_headers, group_id)
    agent_contents = [
        message["content"] for message in messages if message["sender_type"] == "agent"
    ]
    assert (
        "@Helper prepare slide outline\n\nInstructions from @Caller: use the shared brief"
        in agent_contents
    )
    assert "Helper visible answer" in agent_contents


async def test_agent_as_tool_rejects_assistant_not_in_group(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any, tmp_path: Path
) -> None:
    group_id, agents = await _setup(
        client,
        auth_headers,
        ("Caller",),
        free_speech=False,
        proactive_mode=False,
    )
    workspace = await client.post(
        "/api/v1/workspaces",
        headers=auth_headers,
        json={"name": "Helper ws", "backend_type": "local", "local_path": str(tmp_path)},
    )
    assert workspace.status_code == 201, workspace.text
    helper = await client.post(
        "/api/v1/agents",
        headers=auth_headers,
        json={
            "name": "Helper",
            "system_prompt": "You help.",
            "workspace_id": workspace.json()["id"],
        },
    )
    assert helper.status_code == 201, helper.text
    caller_id = agents[0][0]
    helper_id = helper.json()["id"]
    patch = await client.patch(
        f"/api/v1/agents/{caller_id}",
        headers=auth_headers,
        json={"tool_config": {"assistant_agents": [{"agent_id": helper_id, "enabled": True}]}},
    )
    assert patch.status_code == 200, patch.text
    calls = _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="",
                tool_calls=[
                    {
                        "name": "AgentAsTool",
                        "args": {"agent_id": helper_id, "task": "prepare outline"},
                        "id": "agent-tool-1",
                    }
                ],
            ),
            AIMessage(content="Helper must be added first."),
        ],
    )

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Caller delegate this"},
    )

    assert response.status_code == 201, response.text
    assert "must be added to this group" in str(calls[1][-1].content)
    body = response.json()
    assert body["dispatch_messages"] == []
    assert [reply["content"] for reply in body["agent_replies"]] == ["Helper must be added first."]


async def test_direct_agent_as_tool_requires_group_context(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any
) -> None:
    group_id, agents = await _setup(client, auth_headers, ("Caller", "Helper"))
    _ = group_id
    caller_id, helper_id = agents[0][0], agents[1][0]
    patch = await client.patch(
        f"/api/v1/agents/{caller_id}",
        headers=auth_headers,
        json={"tool_config": {"assistant_agents": [{"agent_id": helper_id, "enabled": True}]}},
    )
    assert patch.status_code == 200, patch.text
    calls = _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="",
                tool_calls=[
                    {
                        "name": "AgentAsTool",
                        "args": {"agent_id": helper_id, "task": "prepare outline"},
                        "id": "agent-tool-1",
                    }
                ],
            ),
            AIMessage(content="Cannot privately call helper."),
        ],
    )

    response = await client.post(
        f"/api/v1/agents/{caller_id}/invoke",
        headers=auth_headers,
        json={"message": "delegate"},
    )

    assert response.status_code == 200, response.text
    assert "Cannot privately call helper." in response.json()["content"]
    assert "GROUP_CONTEXT_REQUIRED" in str(calls[1][-1].content)


async def test_agent_as_tool_stream_dispatches_visible_group_message(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any
) -> None:
    group_id, agents = await _setup(
        client,
        auth_headers,
        ("Caller", "Helper"),
        free_speech=False,
        proactive_mode=False,
    )
    caller_id, helper_id = agents[0][0], agents[1][0]
    patch = await client.patch(
        f"/api/v1/agents/{caller_id}",
        headers=auth_headers,
        json={"tool_config": {"assistant_agents": [{"agent_id": helper_id, "enabled": True}]}},
    )
    assert patch.status_code == 200, patch.text
    _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="",
                tool_calls=[
                    {
                        "name": "AgentAsTool",
                        "args": {"agent_id": helper_id, "task": "stream task"},
                        "id": "agent-tool-1",
                    }
                ],
            ),
            AIMessage(content="Stream helper answer"),
        ],
    )

    events = await _stream_events(client, auth_headers, group_id, "@Caller delegate this")
    agent_messages = [json.loads(data) for event, data in events if event == "agent_message"]

    assert [message["content"] for message in agent_messages] == [
        "@Helper stream task",
        "Stream helper answer",
    ]


async def test_agent_as_tool_terminal_handoff_does_not_feed_result_back_to_caller(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any
) -> None:
    group_id, agents = await _setup(
        client,
        auth_headers,
        ("Mike", "Tony"),
        free_speech=False,
        proactive_mode=False,
    )
    mike_id, tony_id = agents[0][0], agents[1][0]
    patch = await client.patch(
        f"/api/v1/agents/{mike_id}",
        headers=auth_headers,
        json={"tool_config": {"assistant_agents": [{"agent_id": tony_id, "enabled": True}]}},
    )
    assert patch.status_code == 200, patch.text
    calls = _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="",
                tool_calls=[
                    {
                        "name": "AgentAsTool",
                        "args": {"agent_id": tony_id, "task": "make the slide outline"},
                        "id": "agent-tool-terminal-1",
                    }
                ],
            ),
            AIMessage(content="Tony separate reply"),
        ],
    )

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Mike ask Tony"},
    )

    assert response.status_code == 201, response.text
    assert len(calls) == 2
    assert not any(
        isinstance(message, ToolMessage) and "DISPATCHED" in str(message.content)
        for message in calls[1]
    )
    assert not any(
        isinstance(message, ToolMessage) and message.tool_call_id == "agent-tool-terminal-1"
        for message in calls[1]
    )
    body = response.json()
    assert [message["sender_id"] for message in body["dispatch_messages"]] == [mike_id]
    assert len(body["dispatch_messages"]) == 1
    assert [message["sender_id"] for message in body["agent_replies"]] == [tony_id]
    history = await _messages(client, auth_headers, group_id)
    agent_history = [message for message in history if message["sender_type"] == "agent"]
    assert [message["sender_id"] for message in agent_history] == [mike_id, tony_id]
    assert agent_history[0]["content"].startswith("@Tony make the slide outline")
    assert all(message["content"] for message in agent_history)


async def test_agent_as_tool_stream_prioritizes_terminal_handoff_over_other_tool_calls(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any, tmp_path: Path
) -> None:
    group_id, agents = await _setup(
        client,
        auth_headers,
        ("Mike", "Tony"),
        free_speech=False,
        proactive_mode=False,
        workspace_path=tmp_path,
    )
    mike_id, tony_id = agents[0][0], agents[1][0]
    patch = await client.patch(
        f"/api/v1/agents/{mike_id}",
        headers=auth_headers,
        json={
            "tool_config": {
                "tools": {
                    "fetch": {"enabled": True},
                    "bash": {"enabled": True},
                    "todo_write": {"enabled": True},
                },
                "assistant_agents": [{"agent_id": tony_id, "enabled": True}],
            }
        },
    )
    assert patch.status_code == 200, patch.text
    calls = _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="I will delegate this and should not become a final answer.",
                tool_calls=[
                    {
                        "name": "Fetch",
                        "args": {"url": "https://example.com/should-not-run"},
                        "id": "fetch-after-handoff",
                    },
                    {
                        "name": "AgentAsTool",
                        "args": {"agent_id": tony_id, "task": "research and make PPT"},
                        "id": "agent-tool-priority-1",
                    },
                    {
                        "name": "Bash",
                        "args": {"command": "pwd"},
                        "id": "bash-after-handoff",
                    },
                ],
            ),
            AIMessage(content="Tony stream reply after handoff"),
        ],
    )

    events = await _stream_events(client, auth_headers, group_id, "@Mike ask Tony")

    assert len(calls) == 2
    assert not any(
        isinstance(message, ToolMessage)
        and message.tool_call_id
        in {"agent-tool-priority-1", "fetch-after-handoff", "bash-after-handoff"}
        for message in calls[1]
    )
    event_names = [event for event, _data in events]
    starts = [json.loads(data) for event, data in events if event == "agent_start"]
    messages = [json.loads(data) for event, data in events if event == "agent_message"]
    result_payloads = [json.loads(data) for event, data in events if event == "tool_call_result"]
    token_payloads = [json.loads(data) for event, data in events if event == "token"]
    assert "agent_handoff" not in event_names
    assert [start["agent_id"] for start in starts] == [mike_id, tony_id]
    assert [payload["tool_name"] for payload in result_payloads] == ["AgentAsTool"]
    assert [payload["tool_call_id"] for payload in result_payloads] == ["agent-tool-priority-1"]
    assert [message["sender_id"] for message in messages] == [mike_id, tony_id]
    assert messages[0]["content"].startswith("@Tony research and make PPT")
    assert messages[1]["content"] == "Tony stream reply after handoff"
    assert all("should not become a final answer" not in message["content"] for message in messages)
    assert any("should not become a final answer" in payload["delta"] for payload in token_payloads)
    assert event_names.index("agent_silent") < event_names.index("agent_message")


async def test_runtime_tool_call_order_is_unchanged_without_handoff_names() -> None:
    from app.agents import runtime

    calls: list[str] = []

    @tool("SecondTool")
    def second_tool() -> str:
        """Record that the second tool ran."""
        calls.append("SecondTool")
        return "second"

    @tool("FirstTool")
    def first_tool() -> str:
        """Record that the first tool ran."""
        calls.append("FirstTool")
        return "first"

    tool_calls = [
        {"name": "SecondTool", "args": {}, "id": "second-1"},
        {"name": "FirstTool", "args": {}, "id": "first-1"},
    ]
    events = [
        (event.tool_name, event.status)
        async for event, _result in runtime._execute_tool_calls(
            tool_calls=tool_calls,
            tools={"FirstTool": first_tool, "SecondTool": second_tool},
            agent_handoff_tool_names=None,
        )
    ]

    assert calls == ["SecondTool", "FirstTool"]
    assert events == [
        ("SecondTool", "started"),
        ("SecondTool", "completed"),
        ("FirstTool", "started"),
        ("FirstTool", "completed"),
    ]


async def test_agent_as_tool_stream_terminal_handoff_emits_tony_separate_turn(
    client: AsyncClient, auth_headers: dict[str, str], monkeypatch: Any
) -> None:
    group_id, agents = await _setup(
        client,
        auth_headers,
        ("Mike", "Tony"),
        free_speech=False,
        proactive_mode=False,
    )
    mike_id, tony_id = agents[0][0], agents[1][0]
    patch = await client.patch(
        f"/api/v1/agents/{mike_id}",
        headers=auth_headers,
        json={"tool_config": {"assistant_agents": [{"agent_id": tony_id, "enabled": True}]}},
    )
    assert patch.status_code == 200, patch.text
    calls = _patch_ai_message_script(
        monkeypatch,
        [
            AIMessage(
                content="",
                tool_calls=[
                    {
                        "name": "AgentAsTool",
                        "args": {"agent_id": tony_id, "task": "make the slide outline"},
                        "id": "agent-tool-stream-terminal-1",
                    }
                ],
            ),
            AIMessage(content="Tony stream reply"),
        ],
    )

    events = await _stream_events(client, auth_headers, group_id, "@Mike ask Tony")

    assert len(calls) == 2
    assert not any(
        isinstance(message, ToolMessage) and message.tool_call_id == "agent-tool-stream-terminal-1"
        for message in calls[1]
    )
    starts = [json.loads(data) for event, data in events if event == "agent_start"]
    messages = [json.loads(data) for event, data in events if event == "agent_message"]
    result_payloads = [json.loads(data) for event, data in events if event == "tool_call_result"]
    assert "agent_handoff" not in [event for event, _data in events]
    assert [start["agent_id"] for start in starts] == [mike_id, tony_id]
    assert len(result_payloads) == 1
    assert result_payloads[0]["tool_name"] == "AgentAsTool"
    assert result_payloads[0]["status"] == "completed"
    assert [message["sender_id"] for message in messages] == [mike_id, tony_id]
    assert messages[0]["content"].startswith("@Tony make the slide outline")
    assert messages[1]["content"] == "Tony stream reply"
    assert all(message["content"] for message in messages)


async def test_stream_visible_reply_budget_caps_replies(
    client: AsyncClient,
    auth_headers: dict[str, str],
    monkeypatch: Any,
) -> None:
    _patch_llm_script(
        monkeypatch,
        ["E1", "M1", "E2", "M2", "E3", "M3", "SHOULD NOT CALL"],
    )
    group_id, _agents = await _setup(
        client,
        auth_headers,
        ("Echo", "Mirror"),
        free_speech=True,
        proactive_reply_multiplier=3,
    )
    monkeypatch.setattr(
        "app.services.message_service.random.sample",
        lambda population, *, k: sorted(population, key=lambda item: item[1].name),
    )

    events = await _stream_events(client, auth_headers, group_id, "hello group")
    event_names = [event for event, _data in events]
    messages = await _messages(client, auth_headers, group_id)
    agent_messages = [message for message in messages if message["sender_type"] == "agent"]

    assert event_names.count("agent_message") == 6
    assert [message["content"] for message in agent_messages] == [
        "E1",
        "M1",
        "E2",
        "M2",
        "E3",
        "M3",
    ]


async def test_stream_rotates_previous_visible_speaker_after_other_candidate(
    client: AsyncClient,
    auth_headers: dict[str, str],
    monkeypatch: Any,
) -> None:
    _patch_llm_script(monkeypatch, ["first 1", "<SILENT>", "other 2", "first 2"])
    group_id, agents = await _setup(
        client,
        auth_headers,
        ("Echo", "Mirror"),
        free_speech=True,
        proactive_reply_multiplier=2,
    )

    def previous_speaker_first_sample(population: Sequence[Any], *, k: int) -> list[Any]:
        assert k == len(population)
        return list(population)

    monkeypatch.setattr(
        "app.services.message_service.random.sample",
        previous_speaker_first_sample,
    )

    events = await _stream_events(client, auth_headers, group_id, "hello group")
    agent_starts = [
        json.loads(data)
        for event, data in events
        if event == "agent_start"
    ]
    messages = await _messages(client, auth_headers, group_id)
    agent_messages = [message for message in messages if message["sender_type"] == "agent"]

    first_agent_id = agent_messages[0]["sender_id"]
    other_agent_id = agents[1][0] if first_agent_id == agents[0][0] else agents[0][0]

    assert [start["agent_id"] for start in agent_starts] == [
        first_agent_id,
        other_agent_id,
        other_agent_id,
        first_agent_id,
        other_agent_id,
        first_agent_id,
    ]

    assert [message["sender_id"] for message in agent_messages] == [
        first_agent_id,
        other_agent_id,
        first_agent_id,
    ]
    assert [message["content"] for message in agent_messages] == [
        "first 1",
        "other 2",
        "first 2",
    ]
