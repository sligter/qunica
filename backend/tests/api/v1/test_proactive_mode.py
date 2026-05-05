from collections.abc import Sequence
from pathlib import Path
from typing import Any, cast

from httpx import AsyncClient
from langchain_core.language_models.fake_chat_models import GenericFakeChatModel
from langchain_core.messages import AIMessage


def _patch_llm_script(monkeypatch: Any, messages: Sequence[str]) -> None:
    script = iter([AIMessage(content=message) for message in messages])

    async def _resolve_factory(_db: Any, _agent: Any, *, streaming: bool = False) -> Any:
        return GenericFakeChatModel(messages=script)

    monkeypatch.setattr("app.services.message_service.resolve_chat_model", _resolve_factory)


async def _setup(
    client: AsyncClient,
    auth_headers: dict[str, str],
    agent_names: Sequence[str] = ("Echo",),
    *,
    free_speech: bool = False,
    proactive_mode: bool = True,
    proactive_max_rounds: int = 1,
) -> tuple[str, list[tuple[str, str]]]:
    workspace = await client.post(
        "/api/v1/workspaces",
        headers=auth_headers,
        json={
            "name": "Proactive repo",
            "backend_type": "local",
            "local_path": str(Path.cwd()),
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
            "proactive_max_rounds": proactive_max_rounds,
        },
    )
    assert patch.status_code == 200, patch.text
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
        proactive_max_rounds=3,
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
        proactive_max_rounds=2,
    )
    monkeypatch.setattr(
        "app.services.message_service.random.sample",
        lambda population, *, k: list(population),
    )

    events = await _stream_events(client, auth_headers, group_id, "hello group")
    event_names = [event for event, _data in events]
    messages = await _messages(client, auth_headers, group_id)
    agent_messages = [message for message in messages if message["sender_type"] == "agent"]

    assert [message["sender_id"] for message in agent_messages] == [agents[0][0], agents[1][0]]
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
        proactive_max_rounds=3,
    )

    events = await _stream_events(client, auth_headers, group_id, "hello group")
    event_names = [event for event, _data in events]
    messages = await _messages(client, auth_headers, group_id)
    agent_messages = [message for message in messages if message["sender_type"] == "agent"]

    assert event_names.count("agent_start") == 4
    assert [message["content"] for message in agent_messages] == ["echo 1", "mirror 1"]


async def test_max_rounds_validation_rejects_out_of_range(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    group_id, _agents = await _setup(client, auth_headers)

    too_low = await client.patch(
        f"/api/v1/groups/{group_id}",
        headers=auth_headers,
        json={"proactive_max_rounds": 0},
    )
    too_high = await client.patch(
        f"/api/v1/groups/{group_id}",
        headers=auth_headers,
        json={"proactive_max_rounds": 6},
    )
    valid = await client.patch(
        f"/api/v1/groups/{group_id}",
        headers=auth_headers,
        json={"proactive_max_rounds": 3},
    )

    assert too_low.status_code == 422
    assert too_high.status_code == 422
    assert valid.status_code == 200
    assert valid.json()["proactive_max_rounds"] == 3


async def test_round_order_join_then_random(
    client: AsyncClient,
    auth_headers: dict[str, str],
    monkeypatch: Any,
) -> None:
    _patch_llm_script(monkeypatch, ["A1", "B1", "C1", "C2", "A2", "B2"])
    group_id, agents = await _setup(
        client,
        auth_headers,
        ("A", "B", "C"),
        free_speech=True,
        proactive_max_rounds=2,
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
        agents[2][0],
        agents[0][0],
        agents[1][0],
    ]
    assert [message["content"] for message in agent_messages] == [
        "A1",
        "B1",
        "C1",
        "C2",
        "A2",
        "B2",
    ]
