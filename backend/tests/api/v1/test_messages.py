import contextlib
import json

from httpx import AsyncClient


async def _setup(
    client: AsyncClient, auth_headers: dict[str, str], extra_agents: list[str] = ()
) -> tuple[str, list[tuple[str, str]]]:
    """Create a group with `Echo` and any additional agents; return
    (group_id, [(agent_id, name), ...])."""
    agents: list[tuple[str, str]] = []
    for name in ("Echo", *extra_agents):
        r = await client.post(
            "/api/v1/agents",
            headers=auth_headers,
            json={"name": name, "system_prompt": f"You are {name}. End with DONE."},
        )
        agents.append((r.json()["id"], name))

    r = await client.post(
        "/api/v1/groups",
        headers=auth_headers,
        json={"name": "MsgGroup", "initial_agents": [a[0] for a in agents]},
    )
    return r.json()["id"], agents


async def test_send_with_mention_triggers_fake_reply_with_thread_id(
    client: AsyncClient, auth_headers: dict[str, str], fake_llm: dict
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
    client: AsyncClient, auth_headers: dict[str, str], fake_llm: dict
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
    client: AsyncClient, auth_headers: dict[str, str], fake_llm: dict
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


async def test_stream_emits_per_agent_attribution(
    client: AsyncClient, auth_headers: dict[str, str], fake_llm: dict
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
    assert agent_message_senders == [echo_id, mirror_id]


async def test_history_lists_persisted_messages_in_order(
    client: AsyncClient, auth_headers: dict[str, str], fake_llm: dict
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
