"""Phase 1 smoke test (cumulative).

Run after `uvicorn app.main:app` is up on :8000 with backend/.env configured.

Phase 1.1 (auth + agent + LLM):
1. /api/v1/health returns ok
2. /auth/register creates a user
3. /auth/login returns a JWT
4. /auth/me echoes the user
5. /agents POST creates an agent
6. /agents GET lists it
7. /agents/{id}/invoke streams a real response from the configured LLM
8. /agents/{id}/invoke/stream emits SSE token events

Phase 1.2 (group + mention-driven messaging):
9.  /groups POST creates a group with the agent pre-attached
10. /groups GET lists groups for the user
11. /groups/{id}/agents GET lists the agent inside the group
12. /groups/{id}/messages POST with `@<agent>` triggers a real LLM reply
13. /groups/{id}/messages POST without @ returns warnings, no agent_replies
14. /groups/{id}/messages/stream emits user_message + tokens + agent_message + done
15. /groups/{id}/messages GET returns the persisted history in order

Manual / human-in-loop verification, not CI.
"""

from __future__ import annotations

import asyncio
import json
import secrets
import sys
from collections.abc import Awaitable, Callable

import httpx
from sqlalchemy import delete, func, or_, select

from app.db import SessionLocal
from app.models.agent import Agent
from app.models.group import Group
from app.models.group_agent import GroupAgent
from app.models.group_file import GroupFile
from app.models.group_member import GroupMember
from app.models.group_note import GroupNote
from app.models.llm_provider import LLMProvider
from app.models.message import Message
from app.models.skill import Skill
from app.models.thread import Thread
from app.models.user import User
from app.models.workspace import Workspace

BASE = "http://127.0.0.1:8000/api/v1"
type CleanupStep = Callable[[], Awaitable[int | None]]


def banner(label: str) -> None:
    print(f"\n=== {label} ===")


async def delete_generated_smoke_users(email: str) -> int:
    """Delete smoke/scratch generated users and their owned data from the active dev DB."""
    async with SessionLocal() as db:
        table_check = (
            await db.execute(
                select(
                    func.to_regclass(GroupFile.__tablename__),
                    func.to_regclass(GroupNote.__tablename__),
                    func.to_regclass(LLMProvider.__tablename__),
                    func.to_regclass(Skill.__tablename__),
                    func.to_regclass(Workspace.__tablename__),
                )
            )
        ).one()
        existing_tables = {table_name for table_name in table_check if table_name is not None}
        users = list(
            await db.scalars(
                select(User).where(
                    or_(
                        User.email == email,
                        (User.name == "Smoke Tester") & User.email.like("smoke-%@example.com"),
                        User.email.op("~")(r"^x-[0-9a-fA-F]+@example\.com$"),
                    )
                )
            )
        )
        if not users:
            return 0

        user_ids = [user.id for user in users]
        owned_groups = list(await db.scalars(select(Group.id).where(Group.owner_id.in_(user_ids))))
        owned_agents = list(await db.scalars(select(Agent.id).where(Agent.owner_id.in_(user_ids))))

        await db.execute(delete(Message).where(Message.sender_id.in_(user_ids)))
        if owned_groups:
            await db.execute(delete(Message).where(Message.group_id.in_(owned_groups)))
            await db.execute(delete(Thread).where(Thread.group_id.in_(owned_groups)))
            if GroupNote.__tablename__ in existing_tables:
                await db.execute(delete(GroupNote).where(GroupNote.group_id.in_(owned_groups)))
            if GroupFile.__tablename__ in existing_tables:
                await db.execute(delete(GroupFile).where(GroupFile.group_id.in_(owned_groups)))
            await db.execute(delete(GroupMember).where(GroupMember.group_id.in_(owned_groups)))
            await db.execute(delete(GroupAgent).where(GroupAgent.group_id.in_(owned_groups)))
            await db.execute(delete(Group).where(Group.id.in_(owned_groups)))
        if owned_agents:
            await db.execute(delete(Message).where(Message.sender_id.in_(owned_agents)))
            await db.execute(delete(Thread).where(Thread.agent_id.in_(owned_agents)))
            await db.execute(delete(GroupAgent).where(GroupAgent.agent_id.in_(owned_agents)))
            await db.execute(delete(Agent).where(Agent.id.in_(owned_agents)))
        if GroupFile.__tablename__ in existing_tables:
            await db.execute(delete(GroupFile).where(GroupFile.uploader_id.in_(user_ids)))
        if GroupNote.__tablename__ in existing_tables:
            await db.execute(delete(GroupNote).where(GroupNote.author_id.in_(user_ids)))
        await db.execute(delete(Thread).where(Thread.created_by.in_(user_ids)))
        await db.execute(delete(GroupMember).where(GroupMember.user_id.in_(user_ids)))
        if LLMProvider.__tablename__ in existing_tables:
            await db.execute(delete(LLMProvider).where(LLMProvider.owner_id.in_(user_ids)))
        if Skill.__tablename__ in existing_tables:
            await db.execute(delete(Skill).where(Skill.owner_id.in_(user_ids)))
        if Workspace.__tablename__ in existing_tables:
            await db.execute(delete(Workspace).where(Workspace.owner_id.in_(user_ids)))
        await db.execute(delete(User).where(User.id.in_(user_ids)))
        await db.commit()
        return len(users)


async def main() -> int:
    suffix = secrets.token_hex(4)
    email = f"smoke-{suffix}@example.com"
    password = "test-password-123"
    cleanup_steps: list[tuple[str, CleanupStep]] = []

    try:
        async with httpx.AsyncClient(base_url=BASE, timeout=60.0) as client:
            banner("health")
            r = await client.get("/health")
            print(r.status_code, r.json())
            assert r.status_code == 200

            banner("register")
            r = await client.post(
                "/auth/register",
                json={"email": email, "password": password, "name": "Smoke Tester"},
            )
            print(r.status_code, r.json())
            assert r.status_code == 201

            banner("login")
            r = await client.post(
                "/auth/login",
                json={"email": email, "password": password},
            )
            print(r.status_code, r.json())
            assert r.status_code == 200
            token: str = r.json()["access_token"]
            auth = {"Authorization": f"Bearer {token}"}
            cleanup_steps.append(
                ("generated smoke users", lambda: delete_generated_smoke_users(email))
            )

            banner("me")
            r = await client.get("/auth/me", headers=auth)
            print(r.status_code, r.json())
            assert r.status_code == 200
            assert r.json()["email"] == email

            banner("create agent")
            r = await client.post(
                "/agents",
                headers=auth,
                json={
                    "name": "Echo",
                    "description": "Phase 1 smoke validator",
                    "system_prompt": (
                        "You are a concise assistant. Reply in one sentence. "
                        "Always end with the word DONE."
                    ),
                },
            )
            print(r.status_code, r.json())
            assert r.status_code == 201
            agent_id: str = r.json()["id"]

            banner("list agents")
            r = await client.get("/agents", headers=auth)
            print(r.status_code, "count =", len(r.json()))
            assert r.status_code == 200
            assert any(a["id"] == agent_id for a in r.json())

            banner("invoke agent (non-stream)")
            r = await client.post(
                f"/agents/{agent_id}/invoke",
                headers=auth,
                json={"message": "Say hello in 3 words."},
            )
            print(r.status_code, r.json())
            assert r.status_code == 200
            assert r.json()["content"].strip()

            banner("invoke agent (SSE stream)")
            async with client.stream(
                "POST",
                f"/agents/{agent_id}/invoke/stream",
                headers=auth,
                json={"message": "Count from 1 to 3."},
            ) as resp:
                print("status", resp.status_code)
                assert resp.status_code == 200
                received = 0
                async for line in resp.aiter_lines():
                    if line.startswith("data:"):
                        received += 1
                        if received <= 8:
                            print(line)
                print(f"... data lines: {received}")
                assert received > 0

            # ---------- Phase 1.2: groups + mention-driven messaging ----------

            banner("create group with initial_agents")
            r = await client.post(
                "/groups",
                headers=auth,
                json={
                    "name": "Smoke Project",
                    "description": "smoke test group",
                    "announcement": "We test in concise English. Reply ends with DONE.",
                    "initial_agents": [agent_id],
                },
            )
            print(r.status_code, r.json())
            assert r.status_code == 201
            group_id: str = r.json()["id"]

            banner("list groups")
            r = await client.get("/groups", headers=auth)
            print(r.status_code, "count =", len(r.json()))
            assert r.status_code == 200
            assert any(g["id"] == group_id for g in r.json())

            banner("list group agents")
            r = await client.get(f"/groups/{group_id}/agents", headers=auth)
            print(r.status_code, r.json())
            assert r.status_code == 200
            assert len(r.json()) == 1
            assert r.json()[0]["agent_id"] == agent_id
            assert r.json()[0]["display_name"] == "Echo"

            banner("send message with @Echo (sync)")
            r = await client.post(
                f"/groups/{group_id}/messages",
                headers=auth,
                json={"content": "@Echo Say hi in 4 words."},
            )
            print(r.status_code, r.json())
            assert r.status_code == 201
            body = r.json()
            assert body["user_message"]["sender_type"] == "user"
            assert len(body["agent_replies"]) == 1
            echo_reply = body["agent_replies"][0]
            assert echo_reply["sender_type"] == "agent"
            assert echo_reply["content"].strip()
            assert body["warnings"] == []
            # Phase 1 Week 3-4: thread_id is now populated (chat_thread for Echo).
            echo_thread_id = echo_reply["thread_id"]
            assert echo_thread_id is not None, "agent reply should have thread_id"

            banner("send message with @Echo again (sync) — thread reuse")
            r = await client.post(
                f"/groups/{group_id}/messages",
                headers=auth,
                json={"content": "@Echo say hi differently"},
            )
            print(r.status_code, r.json())
            assert r.status_code == 201
            echo_reply_2 = r.json()["agent_replies"][0]
            assert echo_reply_2["thread_id"] == echo_thread_id, (
                f"second @Echo should reuse the same chat_thread, "
                f"got {echo_reply_2['thread_id']} vs first {echo_thread_id}"
            )

            banner("GET /threads/{id} returns thread metadata")
            r = await client.get(f"/threads/{echo_thread_id}", headers=auth)
            print(r.status_code, r.json())
            assert r.status_code == 200
            thread = r.json()
            assert thread["thread_type"] == "chat_thread"
            assert thread["agent_id"] == agent_id
            assert thread["group_id"] == group_id
            assert thread["status"] == "completed"

            banner("send message without @ (sync)")
            r = await client.post(
                f"/groups/{group_id}/messages",
                headers=auth,
                json={"content": "no mention here"},
            )
            print(r.status_code, r.json())
            assert r.status_code == 201
            assert r.json()["agent_replies"] == []
            assert r.json()["warnings"] == ["no agent mentioned in this group"]

            # ---------- Phase 1.3: multi-agent fan-out ----------

            banner("create second agent: Mirror")
            r = await client.post(
                "/agents",
                headers=auth,
                json={
                    "name": "Mirror",
                    "description": "Phase 1.3 multi-agent validator",
                    "system_prompt": (
                        "You are Mirror. If a previous assistant message exists in "
                        "this conversation, start your reply with the word 'After:' "
                        "followed by a one-line summary of what they said. Then add "
                        "your own one-sentence reply. Always end with the word DONE."
                    ),
                },
            )
            assert r.status_code == 201
            mirror_agent_id: str = r.json()["id"]

            banner("add Mirror to the group")
            r = await client.post(
                f"/groups/{group_id}/agents",
                headers=auth,
                json={"agent_id": mirror_agent_id},
            )
            print(r.status_code, r.json())
            assert r.status_code == 201
            assert r.json()["display_name"] == "Mirror"

            banner("send @Echo @Mirror — multi-agent fan-out (sync)")
            r = await client.post(
                f"/groups/{group_id}/messages",
                headers=auth,
                json={"content": "@Echo @Mirror Say something brief."},
            )
            print(r.status_code, r.json())
            assert r.status_code == 201
            body = r.json()
            assert body["warnings"] == [], f"expected no warnings, got {body['warnings']}"
            assert len(body["agent_replies"]) == 2, (
                f"expected 2 agent replies (Echo + Mirror), "
                f"got {len(body['agent_replies'])}"
            )
            first, second = body["agent_replies"]
            assert first["sender_id"] == agent_id, "first reply should be Echo"
            assert second["sender_id"] == mirror_agent_id, "second reply should be Mirror"
            # Mirror's chat_thread is its own (different from Echo's).
            mirror_thread_id = second["thread_id"]
            assert mirror_thread_id is not None
            assert mirror_thread_id != echo_thread_id, (
                "Mirror should have its own chat_thread, distinct from Echo's"
            )
            # Cross-agent visibility: Mirror's reply should reference Echo's earlier
            # turn via the "After:" prefix per its system prompt.
            print("Mirror reply text:", second["content"])
            # Don't assert exact prefix match (LLM may not be perfectly literal),
            # but assert non-empty.
            assert second["content"].strip()

            banner("send @Echo @Mirror (SSE stream) — fan-out token attribution")
            events_seen: dict[str, int] = {}
            agent_ids_in_tokens: set[str] = set()
            agent_message_ids: list[str] = []
            async with client.stream(
                "POST",
                f"/groups/{group_id}/messages/stream",
                headers=auth,
                json={"content": "@Echo @Mirror One short reply each please."},
            ) as resp:
                assert resp.status_code == 200
                current_event = ""
                current_data = ""
                async for line in resp.aiter_lines():
                    if line.startswith("event:"):
                        current_event = line.split(":", 1)[1].strip()
                        events_seen[current_event] = events_seen.get(current_event, 0) + 1
                    elif line.startswith("data:"):
                        current_data = line[len("data:") :].strip()
                        if current_event == "token":
                            try:
                                payload = json.loads(current_data)
                                agent_ids_in_tokens.add(payload.get("agent_id", ""))
                            except json.JSONDecodeError:
                                pass
                        elif current_event == "agent_message":
                            try:
                                payload = json.loads(current_data)
                                agent_message_ids.append(payload.get("sender_id", ""))
                            except json.JSONDecodeError:
                                pass
                print("events:", events_seen)
                print("agent_ids in token events:", agent_ids_in_tokens)
                print("agent_message sender order:", agent_message_ids)
                assert events_seen.get("user_message", 0) == 1
                assert events_seen.get("token", 0) > 0
                assert events_seen.get("agent_message", 0) == 2, (
                    "expected 2 agent_message events for multi-@ fan-out, "
                    f"got {events_seen.get('agent_message', 0)}"
                )
                assert events_seen.get("done", 0) == 1
                assert agent_id in agent_ids_in_tokens
                assert mirror_agent_id in agent_ids_in_tokens
                assert agent_message_ids == [agent_id, mirror_agent_id], (
                    "fan-out order should be Echo then Mirror per textual order"
                )

            banner("list group messages (history)")
            r = await client.get(f"/groups/{group_id}/messages", headers=auth)
            print(r.status_code, "count =", len(r.json()))
            assert r.status_code == 200
            msgs = r.json()
            # Sequence so far:
            #   1 sync @Echo:        user + agent          (2)
            #   2 sync @Echo again:  user + agent          (2)
            #   3 sync no-@:         user                  (1)
            #   4 sync @Echo @Mirror:user + agent + agent  (3)
            #   5 stream @Echo @Mirror: user + agent + agent (3)
            # Total = 11
            assert len(msgs) == 11, f"expected 11 messages, got {len(msgs)}"
            assert sum(1 for m in msgs if m["sender_type"] == "user") == 5
            assert sum(1 for m in msgs if m["sender_type"] == "agent") == 6

        banner("ALL OK")
        return 0
    finally:
        while cleanup_steps:
            label, step = cleanup_steps.pop()
            banner(f"cleanup {label}")
            try:
                removed = await step()
                if removed is not None:
                    print(f"removed {removed} {label}")
            except Exception as exc:
                print(f"cleanup {label} failed: {exc}")


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
