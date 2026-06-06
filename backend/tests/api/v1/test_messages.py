import contextlib
import json
from collections.abc import Sequence
from pathlib import Path
from typing import Any, cast
from uuid import UUID

from httpx import AsyncClient
from langchain_core.language_models.fake_chat_models import GenericFakeChatModel
from langchain_core.messages import AIMessage
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.models.message import Message
from app.models.thread import Thread


def _patch_llm_script(monkeypatch: Any, messages: Sequence[str]) -> None:
    script = iter([AIMessage(content=message) for message in messages])

    async def _resolve_factory(_db: Any, _agent: Any, *, streaming: bool = False) -> Any:
        return GenericFakeChatModel(messages=script)

    monkeypatch.setattr("app.services.message_service.resolve_chat_model", _resolve_factory)


async def _setup(
    client: AsyncClient, auth_headers: dict[str, str], extra_agents: Sequence[str] = ()
) -> tuple[str, list[tuple[str, str]]]:
    """Create a group with `Echo` and any additional agents; return
    (group_id, [(agent_id, name), ...])."""
    workspace = await client.post(
        "/api/v1/workspaces",
        headers=auth_headers,
        json={
            "name": "Message repo",
            "backend_type": "local",
            "local_path": str(Path.cwd()),
        },
    )
    assert workspace.status_code == 201, workspace.text
    workspace_id = cast(str, workspace.json()["id"])
    agents: list[tuple[str, str]] = []
    for name in ("Echo", *extra_agents):
        r = await client.post(
            "/api/v1/agents",
            headers=auth_headers,
            json={
                "name": name,
                "system_prompt": f"You are {name}. End with DONE.",
                "workspace_id": workspace_id,
            },
        )
        assert r.status_code == 201, r.text
        agents.append((cast(str, r.json()["id"]), name))

    r = await client.post(
        "/api/v1/groups",
        headers=auth_headers,
        json={
            "name": "MsgGroup",
            "workspace_id": workspace_id,
            "initial_agents": [a[0] for a in agents],
        },
    )
    assert r.status_code == 201, r.text
    return cast(str, r.json()["id"]), agents


async def test_send_with_mention_triggers_fake_reply_with_thread_id(
    client: AsyncClient, auth_headers: dict[str, str], fake_llm: dict[str, Any]
) -> None:
    fake_llm["messages"] = ["sup DONE"]
    group_id, agents = await _setup(client, auth_headers)
    echo_id = agents[0][0]

    r = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo hi"},
    )
    assert r.status_code == 201
    body = r.json()
    assert body["warnings"] == []
    assert len(body["agent_replies"]) == 1
    reply = body["agent_replies"][0]
    assert reply["sender_id"] == echo_id
    assert reply["thread_id"] is not None
    assert reply["content"] == "sup DONE"


async def test_send_without_mention_returns_warning(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    group_id, _ = await _setup(client, auth_headers)
    r = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "hello group"},
    )
    assert r.status_code == 201
    assert r.json()["agent_replies"] == []
    assert r.json()["warnings"] == ["no agent mentioned in this group"]


async def test_send_to_same_agent_twice_reuses_thread(
    client: AsyncClient, auth_headers: dict[str, str], fake_llm: dict[str, Any]
) -> None:
    fake_llm["messages"] = ["one"]
    group_id, _ = await _setup(client, auth_headers)
    r1 = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo first"},
    )
    fake_llm["messages"] = ["two"]
    r2 = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo second"},
    )
    t1 = r1.json()["agent_replies"][0]["thread_id"]
    t2 = r2.json()["agent_replies"][0]["thread_id"]
    assert t1 == t2 and t1 is not None


async def test_multi_mention_fans_out_in_order_with_distinct_threads(
    client: AsyncClient, auth_headers: dict[str, str], fake_llm: dict[str, Any]
) -> None:
    fake_llm["messages"] = ["echo says", "mirror says"]
    group_id, agents = await _setup(client, auth_headers, extra_agents=["Mirror"])
    echo_id, mirror_id = agents[0][0], agents[1][0]

    r = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo @Mirror please reply"},
    )
    assert r.status_code == 201
    body = r.json()
    assert body["warnings"] == []
    assert len(body["agent_replies"]) == 2
    first, second = body["agent_replies"]
    assert first["sender_id"] == echo_id
    assert second["sender_id"] == mirror_id
    assert first["thread_id"] != second["thread_id"]
    assert first["thread_id"] is not None
    assert second["thread_id"] is not None


async def test_agent_reply_mention_triggers_follow_up_turn(
    client: AsyncClient,
    auth_headers: dict[str, str],
    monkeypatch: Any,
) -> None:
    _patch_llm_script(monkeypatch, ["echo opens", "@Echo your turn", "echo follows"])
    group_id, agents = await _setup(client, auth_headers, extra_agents=["Mirror"])
    echo_id, mirror_id = agents[0][0], agents[1][0]
    patch = await client.patch(
        f"/api/v1/groups/{group_id}",
        headers=auth_headers,
        json={"free_speech": True, "proactive_mode": False},
    )
    assert patch.status_code == 200, patch.text

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "please discuss"},
    )

    assert response.status_code == 201, response.text
    replies = response.json()["agent_replies"]
    assert [reply["sender_id"] for reply in replies] == [echo_id, mirror_id, echo_id]
    assert [reply["content"] for reply in replies] == [
        "echo opens",
        "@Echo your turn",
        "echo follows",
    ]


async def test_agent_reply_mention_respects_free_mention_toggle(
    client: AsyncClient,
    auth_headers: dict[str, str],
    monkeypatch: Any,
) -> None:
    _patch_llm_script(monkeypatch, ["echo opens", "@Echo your turn", "should not run"])
    group_id, agents = await _setup(client, auth_headers, extra_agents=["Mirror"])
    echo_id, mirror_id = agents[0][0], agents[1][0]
    patch = await client.patch(
        f"/api/v1/groups/{group_id}",
        headers=auth_headers,
        json={
            "free_speech": True,
            "proactive_mode": False,
            "allow_agent_free_mention": False,
        },
    )
    assert patch.status_code == 200, patch.text

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "please discuss"},
    )

    assert response.status_code == 201, response.text
    replies = response.json()["agent_replies"]
    assert [reply["sender_id"] for reply in replies] == [echo_id, mirror_id]
    assert [reply["content"] for reply in replies] == ["echo opens", "@Echo your turn"]


async def test_agent_reply_mention_respects_custom_follow_up_limit(
    client: AsyncClient,
    auth_headers: dict[str, str],
    monkeypatch: Any,
) -> None:
    _patch_llm_script(
        monkeypatch,
        ["echo opens", "@Echo your turn", "@Mirror again", "should not run"],
    )
    group_id, agents = await _setup(client, auth_headers, extra_agents=["Mirror"])
    echo_id, mirror_id = agents[0][0], agents[1][0]
    patch = await client.patch(
        f"/api/v1/groups/{group_id}",
        headers=auth_headers,
        json={
            "free_speech": True,
            "proactive_mode": False,
            "agent_free_mention_max_dispatches": 1,
        },
    )
    assert patch.status_code == 200, patch.text
    assert patch.json()["agent_free_mention_max_dispatches"] == 1

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "please discuss"},
    )

    assert response.status_code == 201, response.text
    replies = response.json()["agent_replies"]
    assert [reply["sender_id"] for reply in replies] == [echo_id, mirror_id, echo_id]
    assert [reply["content"] for reply in replies] == [
        "echo opens",
        "@Echo your turn",
        "@Mirror again",
    ]


async def test_agent_reply_mention_limit_zero_disables_follow_up(
    client: AsyncClient,
    auth_headers: dict[str, str],
    monkeypatch: Any,
) -> None:
    _patch_llm_script(monkeypatch, ["echo opens", "@Echo your turn", "should not run"])
    group_id, agents = await _setup(client, auth_headers, extra_agents=["Mirror"])
    echo_id, mirror_id = agents[0][0], agents[1][0]
    patch = await client.patch(
        f"/api/v1/groups/{group_id}",
        headers=auth_headers,
        json={
            "free_speech": True,
            "proactive_mode": False,
            "agent_free_mention_max_dispatches": 0,
        },
    )
    assert patch.status_code == 200, patch.text

    response = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "please discuss"},
    )

    assert response.status_code == 201, response.text
    replies = response.json()["agent_replies"]
    assert [reply["sender_id"] for reply in replies] == [echo_id, mirror_id]
    assert [reply["content"] for reply in replies] == ["echo opens", "@Echo your turn"]


async def test_stream_emits_per_agent_attribution(
    client: AsyncClient, auth_headers: dict[str, str], fake_llm: dict[str, Any]
) -> None:
    fake_llm["messages"] = ["e1", "m1"]
    group_id, agents = await _setup(client, auth_headers, extra_agents=["Mirror"])
    echo_id, mirror_id = agents[0][0], agents[1][0]

    events_seen: dict[str, int] = {}
    token_agent_ids: set[str] = set()
    agent_message_senders: list[str] = []
    async with client.stream(
        "POST",
        f"/api/v1/groups/{group_id}/messages/stream",
        headers=auth_headers,
        json={"content": "@Echo @Mirror go"},
    ) as resp:
        assert resp.status_code == 200
        current_event = ""
        async for line in resp.aiter_lines():
            if line.startswith("event:"):
                current_event = line.split(":", 1)[1].strip()
                events_seen[current_event] = events_seen.get(current_event, 0) + 1
            elif line.startswith("data:"):
                data = line[len("data:") :].strip()
                if current_event == "token" and data:
                    with contextlib.suppress(json.JSONDecodeError):
                        token_agent_ids.add(json.loads(data).get("agent_id", ""))
                elif current_event == "agent_message" and data:
                    with contextlib.suppress(json.JSONDecodeError):
                        agent_message_senders.append(
                            json.loads(data).get("sender_id", "")
                        )

    assert events_seen.get("user_message", 0) == 1
    assert events_seen.get("token", 0) > 0
    assert events_seen.get("agent_message", 0) == 2
    assert events_seen.get("done", 0) == 1
    assert echo_id in token_agent_ids
    assert mirror_id in token_agent_ids
    assert set(agent_message_senders) == {echo_id, mirror_id}


async def test_stream_agent_reply_mention_triggers_follow_up_turn(
    client: AsyncClient,
    auth_headers: dict[str, str],
    monkeypatch: Any,
) -> None:
    _patch_llm_script(monkeypatch, ["echo opens", "@Echo your turn", "echo follows"])
    group_id, agents = await _setup(client, auth_headers, extra_agents=["Mirror"])
    echo_id, mirror_id = agents[0][0], agents[1][0]
    patch = await client.patch(
        f"/api/v1/groups/{group_id}",
        headers=auth_headers,
        json={"free_speech": True, "proactive_mode": False},
    )
    assert patch.status_code == 200, patch.text

    events: list[tuple[str, str]] = []
    current_event = ""
    async with client.stream(
        "POST",
        f"/api/v1/groups/{group_id}/messages/stream",
        headers=auth_headers,
        json={"content": "please discuss"},
    ) as resp:
        assert resp.status_code == 200
        async for line in resp.aiter_lines():
            if line.startswith("event:"):
                current_event = line.split(":", 1)[1].strip()
            elif line.startswith("data:"):
                events.append((current_event, line[len("data:") :].strip()))

    agent_starts = [json.loads(data) for event, data in events if event == "agent_start"]
    agent_messages = [
        json.loads(data) for event, data in events if event == "agent_message"
    ]

    assert [start["agent_id"] for start in agent_starts] == [echo_id, mirror_id, echo_id]
    assert [message["sender_id"] for message in agent_messages] == [
        echo_id,
        mirror_id,
        echo_id,
    ]
    assert [message["content"] for message in agent_messages] == [
        "echo opens",
        "@Echo your turn",
        "echo follows",
    ]


async def test_stream_mesh_runs_agents_sequentially_so_later_agents_see_prior_replies(
    client: AsyncClient, auth_headers: dict[str, str], fake_llm: dict[str, Any]
) -> None:
    fake_llm["messages"] = ["echo reply", "mirror reply"]
    group_id, agents = await _setup(client, auth_headers, extra_agents=["Mirror"])
    echo_id, mirror_id = agents[0][0], agents[1][0]

    events: list[tuple[str, str]] = []
    current_event = ""
    async with client.stream(
        "POST",
        f"/api/v1/groups/{group_id}/messages/stream",
        headers=auth_headers,
        json={"content": "@Echo @Mirror go"},
    ) as resp:
        assert resp.status_code == 200
        async for line in resp.aiter_lines():
            if line.startswith("event:"):
                current_event = line.split(":", 1)[1].strip()
            elif line.startswith("data:"):
                events.append((current_event, line[len("data:") :].strip()))

    event_names = [event for event, _data in events]
    first_start_index = event_names.index("agent_start")
    first_reply_index = event_names.index("agent_message")
    second_start_index = event_names.index("agent_start", first_start_index + 1)
    second_reply_index = event_names.index("agent_message", first_reply_index + 1)
    start_payloads = [json.loads(data) for event, data in events if event == "agent_start"]
    message_payloads = [json.loads(data) for event, data in events if event == "agent_message"]

    assert first_start_index < first_reply_index < second_start_index < second_reply_index
    assert [payload["agent_id"] for payload in start_payloads] == [echo_id, mirror_id]
    assert [payload["sender_id"] for payload in message_payloads] == [echo_id, mirror_id]
    assert len({payload["stream_id"] for payload in start_payloads}) == 1


async def test_clear_group_history_hides_messages_and_allows_new_send(
    client: AsyncClient,
    auth_headers: dict[str, str],
    fake_llm: dict[str, Any],
    db_session: AsyncSession,
) -> None:
    fake_llm["messages"] = ["one", "two"]
    group_id, _ = await _setup(client, auth_headers)
    await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo round 1"},
    )

    r = await client.post(f"/api/v1/groups/{group_id}/messages/clear", headers=auth_headers)
    assert r.status_code == 200
    assert r.json()["cleared_count"] == 2

    r = await client.get(f"/api/v1/groups/{group_id}/messages", headers=auth_headers)
    assert r.status_code == 200
    assert r.json() == []

    cleared_messages = list(
        await db_session.scalars(select(Message).where(Message.group_id == group_id))
    )
    assert len(cleared_messages) == 2
    assert {message.status for message in cleared_messages} == {"cleared"}

    r = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo round 2"},
    )
    assert r.status_code == 201
    assert len(r.json()["agent_replies"]) == 1


async def test_clear_group_history_refuses_when_visible_thread_is_running(
    client: AsyncClient,
    auth_headers: dict[str, str],
    fake_llm: dict[str, Any],
    db_session: AsyncSession,
) -> None:
    fake_llm["messages"] = ["one"]
    group_id, _ = await _setup(client, auth_headers)
    await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo round 1"},
    )
    message = await db_session.scalar(
        select(Message).where(Message.group_id == UUID(group_id), Message.sender_type == "agent")
    )
    assert message is not None
    thread = await db_session.scalar(select(Thread).where(Thread.id == message.thread_id))
    assert thread is not None
    thread.status = "running"
    await db_session.flush()

    r = await client.post(f"/api/v1/groups/{group_id}/messages/clear", headers=auth_headers)
    assert r.status_code == 409

    r = await client.get(f"/api/v1/groups/{group_id}/messages", headers=auth_headers)
    assert r.status_code == 200
    assert len(r.json()) == 2


async def test_clear_group_history_clears_stale_running_threads_with_visible_messages(
    client: AsyncClient,
    auth_headers: dict[str, str],
    fake_llm: dict[str, Any],
    db_session: AsyncSession,
) -> None:
    fake_llm["messages"] = ["one"]
    group_id, _ = await _setup(client, auth_headers)
    await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo round 1"},
    )
    message = await db_session.scalar(
        select(Message).where(Message.group_id == UUID(group_id), Message.sender_type == "agent")
    )
    assert message is not None
    assert message.thread_id is not None
    thread = await db_session.scalar(select(Thread).where(Thread.id == message.thread_id))
    assert thread is not None
    message.status = "cleared"
    thread.status = "running"
    await db_session.flush()

    fake_llm["messages"] = ["two"]
    await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo round 2"},
    )

    r = await client.post(f"/api/v1/groups/{group_id}/messages/clear", headers=auth_headers)
    assert r.status_code == 200, r.text
    assert r.json()["cleared_count"] == 3
    await db_session.flush()

    r = await client.get(f"/api/v1/groups/{group_id}/messages", headers=auth_headers)
    assert r.status_code == 200
    assert r.json() == []
    await db_session.refresh(thread)
    assert thread.status == "cleared"


async def test_clear_group_history_marks_paused_interrupted_threads_cleared(
    client: AsyncClient,
    auth_headers: dict[str, str],
    fake_llm: dict[str, Any],
    db_session: AsyncSession,
) -> None:
    fake_llm["messages"] = ["one"]
    group_id, _ = await _setup(client, auth_headers)
    await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo round 1"},
    )
    message = await db_session.scalar(
        select(Message).where(Message.group_id == group_id, Message.sender_type == "agent")
    )
    assert message is not None
    thread = await db_session.scalar(select(Thread).where(Thread.id == message.thread_id))
    assert thread is not None
    message.status = "interrupted"
    thread.status = "paused"
    await db_session.flush()

    r = await client.post(f"/api/v1/groups/{group_id}/messages/clear", headers=auth_headers)
    assert r.status_code == 200
    assert r.json()["cleared_count"] == 2

    await db_session.refresh(message)
    await db_session.refresh(thread)
    assert message.status == "cleared"
    assert thread.status == "cleared"


async def test_clear_group_history_does_not_clear_group_members_agents_or_workspace(
    client: AsyncClient,
    auth_headers: dict[str, str],
    fake_llm: dict[str, Any],
    db_session: AsyncSession,
) -> None:
    fake_llm["messages"] = ["one"]
    group_id, agents = await _setup(client, auth_headers)
    echo_id = agents[0][0]
    await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo round 1"},
    )
    before_group = await client.get(f"/api/v1/groups/{group_id}", headers=auth_headers)
    assert before_group.status_code == 200
    workspace_id = before_group.json()["workspace_id"]

    r = await client.post(f"/api/v1/groups/{group_id}/messages/clear", headers=auth_headers)
    assert r.status_code == 200

    after_group = await client.get(f"/api/v1/groups/{group_id}", headers=auth_headers)
    assert after_group.status_code == 200
    assert after_group.json()["workspace_id"] == workspace_id
    members = await client.get(f"/api/v1/groups/{group_id}/members", headers=auth_headers)
    assert members.status_code == 200
    assert len(members.json()) == 1
    group_agents = await client.get(f"/api/v1/groups/{group_id}/agents", headers=auth_headers)
    assert group_agents.status_code == 200
    assert [agent["agent_id"] for agent in group_agents.json()] == [echo_id]
    assert await db_session.scalar(select(Message).where(Message.group_id == UUID(group_id)))


async def test_non_owner_cannot_clear_group_history(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    group_id, _ = await _setup(client, auth_headers)
    r = await client.post(
        "/api/v1/auth/register",
        json={
            "email": "member-clear@example.com",
            "password": "valid-password-1",
            "name": "Member",
        },
    )
    assert r.status_code == 201
    login = await client.post(
        "/api/v1/auth/login",
        json={"email": "member-clear@example.com", "password": "valid-password-1"},
    )
    member_headers = {"Authorization": f"Bearer {login.json()['access_token']}"}
    me = await client.get("/api/v1/auth/me", headers=member_headers)
    await client.post(
        f"/api/v1/groups/{group_id}/members",
        headers=auth_headers,
        json={"user_id": me.json()["id"]},
    )

    r = await client.post(f"/api/v1/groups/{group_id}/messages/clear", headers=member_headers)
    assert r.status_code == 403


async def test_history_lists_persisted_messages_in_order(
    client: AsyncClient, auth_headers: dict[str, str], fake_llm: dict[str, Any]
) -> None:
    fake_llm["messages"] = ["one", "two"]
    group_id, _ = await _setup(client, auth_headers)
    await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo round 1"},
    )
    await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo round 2"},
    )
    r = await client.get(
        f"/api/v1/groups/{group_id}/messages", headers=auth_headers
    )
    assert r.status_code == 200
    senders = [m["sender_type"] for m in r.json()]
    assert senders == ["user", "agent", "user", "agent"]


async def test_history_defaults_to_latest_thirty_and_paginates_older_messages(
    client: AsyncClient, auth_headers: dict[str, str], fake_llm: dict[str, Any]
) -> None:
    fake_llm["messages"] = [f"reply {index}" for index in range(36)]
    group_id, _ = await _setup(client, auth_headers)
    for index in range(36):
        r = await client.post(
            f"/api/v1/groups/{group_id}/messages",
            headers=auth_headers,
            json={"content": f"@Echo round {index}"},
        )
        assert r.status_code == 201, r.text

    r = await client.get(f"/api/v1/groups/{group_id}/messages", headers=auth_headers)
    assert r.status_code == 200
    first_page = r.json()
    assert len(first_page) == 30
    assert first_page[0]["content"] == "@Echo round 21"
    assert first_page[-2]["content"] == "@Echo round 35"

    r = await client.get(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        params={"limit": 30, "before": first_page[0]["id"]},
    )
    assert r.status_code == 200
    second_page = r.json()
    assert len(second_page) == 30
    assert second_page[0]["content"] == "@Echo round 6"
    assert second_page[-2]["content"] == "@Echo round 20"
