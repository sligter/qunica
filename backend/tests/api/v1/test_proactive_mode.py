import json
from collections.abc import Sequence
from pathlib import Path
from typing import Any, ClassVar, cast

from httpx import AsyncClient
from langchain_core.language_models.fake_chat_models import GenericFakeChatModel
from langchain_core.messages import AIMessage, BaseMessage, ToolMessage
from pydantic import Field


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


async def test_stream_native_tool_loop_emits_final_answer_without_placeholder(
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

    assert "The workspace contains notes.txt." in "".join(tokens)
    assert agent_messages[0]["content"] == "The workspace contains notes.txt."
    assert "Non-executed tool markup removed" not in agent_messages[0]["content"]


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
