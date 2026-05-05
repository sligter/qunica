from collections.abc import Sequence
from pathlib import Path
from typing import Any, ClassVar, cast

from httpx import AsyncClient
from langchain_core.language_models.fake_chat_models import GenericFakeChatModel
from langchain_core.messages import AIMessage, BaseMessage, ToolMessage
from pydantic import Field

JsonObject = dict[str, Any]


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


async def _create_workspace(client: AsyncClient, headers: dict[str, str]) -> JsonObject:
    r = await client.post(
        "/api/v1/workspaces",
        headers=headers,
        json={
            "name": "Repo",
            "backend_type": "local",
            "local_path": str(Path.cwd()),
        },
    )
    assert r.status_code == 201, r.text
    return cast(JsonObject, r.json())


async def _create_agent(
    client: AsyncClient, headers: dict[str, str], name: str = "Echo"
) -> JsonObject:
    workspace = await _create_workspace(client, headers)
    r = await client.post(
        "/api/v1/agents",
        headers=headers,
        json={
            "name": name,
            "description": f"{name} description",
            "system_prompt": f"You are {name}. End with DONE.",
            "workspace_id": workspace["id"],
        },
    )
    assert r.status_code == 201, r.text
    return cast(JsonObject, r.json())


async def test_create_agent_returns_201(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    a = await _create_agent(client, auth_headers, name="Nova")
    assert a["name"] == "Nova"
    assert a["visibility"] == "private"
    assert a["status"] == "active"
    assert a["workspace_id"] is not None
    assert a["tool_config"]["tools"]["read"]["enabled"] is True


async def test_tool_catalog_returns_builtin_tools(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    r = await client.get("/api/v1/agents/tool-catalog", headers=auth_headers)
    assert r.status_code == 200
    tool_ids = {tool["id"] for tool in r.json()["tools"]}
    expected = {
        "read",
        "write",
        "edit",
        "glob",
        "grep",
        "bash",
        "ask_user",
        "web_search",
        "fetch",
        "run_sub_agent",
        "generate_image",
        "generate_video",
        "skill_manager",
        "todo_write",
        "exit_plan_mode",
    }
    assert expected == tool_ids
    status_by_id = {tool["id"]: tool["runtime_status"] for tool in r.json()["tools"]}
    assert status_by_id["read"] == "available"
    assert status_by_id["write"] == "available"
    assert status_by_id["edit"] == "available"
    assert status_by_id["glob"] == "available"
    assert status_by_id["grep"] == "available"
    assert status_by_id["bash"] == "available"
    assert status_by_id["fetch"] == "available"
    assert all(status == "available" for status in status_by_id.values())


async def test_create_agent_requires_workspace(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    r = await client.post(
        "/api/v1/agents",
        headers=auth_headers,
        json={
            "name": "NoWorkspace",
            "system_prompt": "You are missing a workspace.",
        },
    )
    assert r.status_code == 422


async def test_create_agent_rejects_unknown_tool(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    workspace = await _create_workspace(client, auth_headers)
    r = await client.post(
        "/api/v1/agents",
        headers=auth_headers,
        json={
            "name": "BadTool",
            "system_prompt": "You are BadTool.",
            "workspace_id": workspace["id"],
            "tool_config": {"tools": {"made_up": {"enabled": True}}},
        },
    )
    assert r.status_code == 400


async def test_list_agents_only_returns_own(client: AsyncClient) -> None:
    # User 1 creates an agent
    import secrets

    s1 = secrets.token_hex(4)
    email1 = f"u1-{s1}@example.com"
    await client.post(
        "/api/v1/auth/register",
        json={"email": email1, "password": "valid-password-1", "name": "U1"},
    )
    r = await client.post(
        "/api/v1/auth/login",
        json={"email": email1, "password": "valid-password-1"},
    )
    h1 = {"Authorization": f"Bearer {r.json()['access_token']}"}
    await _create_agent(client, h1, name="OnlyForU1")

    # User 2 has none
    s2 = secrets.token_hex(4)
    email2 = f"u2-{s2}@example.com"
    await client.post(
        "/api/v1/auth/register",
        json={"email": email2, "password": "valid-password-1", "name": "U2"},
    )
    r = await client.post(
        "/api/v1/auth/login",
        json={"email": email2, "password": "valid-password-1"},
    )
    h2 = {"Authorization": f"Bearer {r.json()['access_token']}"}

    r = await client.get("/api/v1/agents", headers=h1)
    assert r.status_code == 200
    assert any(a["name"] == "OnlyForU1" for a in r.json())

    r = await client.get("/api/v1/agents", headers=h2)
    assert r.status_code == 200
    assert all(a["name"] != "OnlyForU1" for a in r.json())


async def test_get_other_users_agent_forbidden(client: AsyncClient) -> None:
    import secrets

    s1 = secrets.token_hex(4)
    email1 = f"o1-{s1}@example.com"
    await client.post(
        "/api/v1/auth/register",
        json={"email": email1, "password": "valid-password-1", "name": "O1"},
    )
    r = await client.post(
        "/api/v1/auth/login",
        json={"email": email1, "password": "valid-password-1"},
    )
    h1 = {"Authorization": f"Bearer {r.json()['access_token']}"}
    a = await _create_agent(client, h1, name="Private")

    s2 = secrets.token_hex(4)
    email2 = f"o2-{s2}@example.com"
    await client.post(
        "/api/v1/auth/register",
        json={"email": email2, "password": "valid-password-1", "name": "O2"},
    )
    r = await client.post(
        "/api/v1/auth/login",
        json={"email": email2, "password": "valid-password-1"},
    )
    h2 = {"Authorization": f"Bearer {r.json()['access_token']}"}

    r = await client.get(f"/api/v1/agents/{a['id']}", headers=h2)
    assert r.status_code == 403


async def test_direct_invoke_loops_until_no_tool_call(
    client: AsyncClient,
    auth_headers: dict[str, str],
    monkeypatch: Any,
) -> None:
    calls: list[list[BaseMessage]] = []
    RecordingFakeChatModel.shared_calls = calls
    script = iter(
        [
            AIMessage(
                content="",
                tool_calls=[{"name": "Glob", "args": {"pattern": "*"}, "id": "glob-1"}],
            ),
            AIMessage(
                content="",
                tool_calls=[
                    {"name": "Read", "args": {"file_path": "pyproject.toml"}, "id": "read-1"}
                ],
            ),
            AIMessage(content="I used both tools."),
        ]
    )

    async def _resolve_factory(_db: Any, _agent: Any, *, streaming: bool = False) -> Any:
        _ = streaming
        return RecordingFakeChatModel(messages=script)

    monkeypatch.setattr("app.api.v1.agents.resolve_chat_model", _resolve_factory)
    a = await _create_agent(client, auth_headers, name="Toolable")

    r = await client.post(
        f"/api/v1/agents/{a['id']}/invoke",
        headers=auth_headers,
        json={"message": "list files"},
    )

    assert r.status_code == 200, r.text
    assert r.json()["content"] == "I used both tools."
    assert len(calls) == 3
    assert sum(isinstance(message, ToolMessage) for message in calls[1]) == 1
    assert sum(isinstance(message, ToolMessage) for message in calls[2]) == 2


async def test_invoke_uses_fake_llm(
    client: AsyncClient,
    auth_headers: dict[str, str],
    fake_llm: dict[str, Any],
) -> None:
    fake_llm["messages"] = ["hello from fake"]
    a = await _create_agent(client, auth_headers, name="Fakeable")
    r = await client.post(
        f"/api/v1/agents/{a['id']}/invoke",
        headers=auth_headers,
        json={"message": "anything"},
    )
    assert r.status_code == 200
    assert r.json()["content"] == "hello from fake"


async def test_invoke_stream_emits_token_and_done(
    client: AsyncClient,
    auth_headers: dict[str, str],
    fake_llm: dict[str, Any],
) -> None:
    fake_llm["messages"] = ["streamed reply"]
    a = await _create_agent(client, auth_headers, name="Streamable")
    events = []
    async with client.stream(
        "POST",
        f"/api/v1/agents/{a['id']}/invoke/stream",
        headers=auth_headers,
        json={"message": "tell me"},
    ) as resp:
        assert resp.status_code == 200
        async for line in resp.aiter_lines():
            if line.startswith("event:"):
                events.append(line.split(":", 1)[1].strip())
    assert "token" in events
    assert "done" in events
