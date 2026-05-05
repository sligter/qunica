"""Regression tests for `messages.created_at` ordering.

Bug: PostgreSQL's `now()` returns the transaction start time, so a
user message + an agent reply persisted within the same
`send_message_stream` request transaction would get identical
`created_at` values. `ORDER BY created_at ASC` then returned them in
PG-implementation-defined order, sometimes placing the agent reply
before the user message that triggered it. The visible symptom was the
new agent bubble rendering above the user's `@` message in the chat
list, sometimes appearing nested with a previous agent's reply card.

Fix: `messages.created_at` now defaults to `clock_timestamp()`
(statement time), so each INSERT in the same transaction gets a
distinct timestamp. A secondary `Message.id` sort key in
`list_messages` keeps legacy rows (with colliding `now()` timestamps)
deterministically ordered.

These tests assert both invariants: (a) the API returns user before
agent, and (b) the persisted `created_at` values are strictly
increasing across the same transaction.
"""
from collections.abc import Sequence
from datetime import datetime
from pathlib import Path
from typing import Any, cast

from httpx import AsyncClient


async def _setup(
    client: AsyncClient, auth_headers: dict[str, str], extra_agents: Sequence[str] = ()
) -> tuple[str, list[tuple[str, str]]]:
    """Create a workspace + group with an `Echo` agent (and optional extras)."""
    workspace = await client.post(
        "/api/v1/workspaces",
        headers=auth_headers,
        json={
            "name": "Ordering repo",
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
            "name": "OrderingGroup",
            "workspace_id": workspace_id,
            "initial_agents": [a[0] for a in agents],
        },
    )
    assert r.status_code == 201, r.text
    return cast(str, r.json()["id"]), agents


async def test_user_and_agent_share_transaction_yet_get_distinct_created_at(
    client: AsyncClient, auth_headers: dict[str, str], fake_llm: dict[str, Any]
) -> None:
    """A single `@Echo` send persists user_msg + agent_msg in the same
    request transaction. With the fix in place they MUST get distinct
    `created_at` values (statement time, not transaction start)."""
    fake_llm["messages"] = ["reply DONE"]
    group_id, _ = await _setup(client, auth_headers)

    r = await client.post(
        f"/api/v1/groups/{group_id}/messages",
        headers=auth_headers,
        json={"content": "@Echo hi"},
    )
    assert r.status_code == 201, r.text

    r = await client.get(
        f"/api/v1/groups/{group_id}/messages", headers=auth_headers
    )
    assert r.status_code == 200, r.text
    msgs = r.json()
    assert len(msgs) == 2, msgs
    user_msg, agent_msg = msgs[0], msgs[1]
    assert user_msg["sender_type"] == "user"
    assert agent_msg["sender_type"] == "agent"

    user_ts = datetime.fromisoformat(user_msg["created_at"])
    agent_ts = datetime.fromisoformat(agent_msg["created_at"])
    assert user_ts < agent_ts, (
        "user_msg.created_at must be strictly less than agent_msg.created_at "
        "for messages persisted in the same transaction. With the buggy "
        f"`now()` default they tied at {user_ts!s}; with the fixed "
        f"`clock_timestamp()` default they should differ. "
        f"Got user={user_ts!s}, agent={agent_ts!s}."
    )


async def test_history_order_stable_across_multiple_sends(
    client: AsyncClient, auth_headers: dict[str, str], fake_llm: dict[str, Any]
) -> None:
    """Multiple sequential `@Echo` sends each persist (user, agent) pairs
    in their own transaction. The full history must come back as
    user/agent/user/agent in chronological order — never with an agent
    reply preceding its triggering user message."""
    fake_llm["messages"] = ["one", "two", "three"]
    group_id, _ = await _setup(client, auth_headers)

    for i in range(3):
        r = await client.post(
            f"/api/v1/groups/{group_id}/messages",
            headers=auth_headers,
            json={"content": f"@Echo round {i}"},
        )
        assert r.status_code == 201, r.text

    r = await client.get(
        f"/api/v1/groups/{group_id}/messages", headers=auth_headers
    )
    assert r.status_code == 200, r.text
    msgs = r.json()
    assert len(msgs) == 6, msgs
    senders = [m["sender_type"] for m in msgs]
    assert senders == ["user", "agent", "user", "agent", "user", "agent"], (
        f"history must alternate user/agent in chronological order; got {senders}"
    )

    # Timestamps must be non-decreasing across the entire history; with
    # the fix they should be strictly increasing.
    timestamps = [datetime.fromisoformat(m["created_at"]) for m in msgs]
    for prev, curr in zip(timestamps, timestamps[1:], strict=False):
        assert prev < curr, (
            f"timestamps must be strictly increasing across same-transaction "
            f"and across-transaction inserts; got {prev!s} >= {curr!s}"
        )
