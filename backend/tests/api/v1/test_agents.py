import sys
from collections.abc import Sequence
from pathlib import Path
from typing import Any, ClassVar, cast

from httpx import AsyncClient
from langchain_core.language_models.fake_chat_models import GenericFakeChatModel
from langchain_core.messages import AIMessage, BaseMessage, ToolMessage
from pydantic import Field

from app.agents.defaults import DEFAULT_AGENT_SYSTEM_PROMPT
from app.api.v1 import agents as agents_api
from app.external_agents.discovery import AcpRuntimeChoice, AcpRuntimePreset

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


async def test_acp_runtime_presets_serializes_discovered_dataclasses(
    monkeypatch: Any,
) -> None:
    def _discover() -> list[AcpRuntimePreset]:
        return [
            AcpRuntimePreset(
                id="codex",
                name="Codex",
                description="Codex adapter",
                profile="codex",
                installed=True,
                command="codex-acp.cmd",
                default_mode="read-only",
                model_options=[AcpRuntimeChoice("", "Default")],
                install_hint="Install Codex ACP.",
                source="PATH",
            )
        ]

    monkeypatch.setattr(agents_api, "discover_acp_runtime_presets", _discover)

    response = await agents_api.get_acp_runtime_presets()

    body = response.model_dump()
    assert body["presets"][0]["id"] == "codex"
    assert body["presets"][0]["command"] == "codex-acp.cmd"
    assert body["presets"][0]["model_options"] == [
        {"value": "", "label": "Default", "description": None}
    ]


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


async def test_create_agent_uses_default_system_prompt(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    workspace = await _create_workspace(client, auth_headers)
    r = await client.post(
        "/api/v1/agents",
        headers=auth_headers,
        json={
            "name": "DefaultPrompt",
            "workspace_id": workspace["id"],
        },
    )

    assert r.status_code == 201, r.text
    assert r.json()["system_prompt"] == DEFAULT_AGENT_SYSTEM_PROMPT


async def test_create_agent_accepts_acp_runtime(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    workspace = await _create_workspace(client, auth_headers)
    r = await client.post(
        "/api/v1/agents",
        headers=auth_headers,
        json={
            "name": "ACP",
            "workspace_id": workspace["id"],
            "runtime_kind": "acp",
            "acp_runtime": {
                "command": sys.executable,
                "args": ["agent.py", "--acp"],
                "timeout_seconds": 10,
                "permission_policy": "deny",
            },
        },
    )

    assert r.status_code == 201, r.text
    body = r.json()
    assert body["runtime_kind"] == "acp"
    assert body["acp_runtime"]["command"] == sys.executable
    assert body["acp_runtime"]["args"] == ["agent.py", "--acp"]
    assert body["llm_provider_id"] is None


async def test_create_agent_rejects_external_cli_runtime(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    workspace = await _create_workspace(client, auth_headers)
    r = await client.post(
        "/api/v1/agents",
        headers=auth_headers,
        json={
            "name": "Old CLI",
            "workspace_id": workspace["id"],
            "runtime_kind": "external_cli",
            "external_runtime": {"adapter": "codex"},
        },
    )

    assert r.status_code == 422


async def test_create_agent_accepts_temperature_005_increment(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    workspace = await _create_workspace(client, auth_headers)
    r = await client.post(
        "/api/v1/agents",
        headers=auth_headers,
        json={
            "name": "FineTemp",
            "system_prompt": "Use exact temperature.",
            "workspace_id": workspace["id"],
            "llm_config": {"temperature": 0.05},
        },
    )

    assert r.status_code == 201, r.text
    assert r.json()["llm_config"]["temperature"] == 0.05


async def test_create_agent_drops_max_tokens_from_llm_config(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    workspace = await _create_workspace(client, auth_headers)
    r = await client.post(
        "/api/v1/agents",
        headers=auth_headers,
        json={
            "name": "NoMaxTokens",
            "system_prompt": "Do not cap output.",
            "workspace_id": workspace["id"],
            "llm_config": {"temperature": 0.05, "max_tokens": 4096},
        },
    )

    assert r.status_code == 201, r.text
    assert r.json()["llm_config"] == {"temperature": 0.05}


async def test_create_agent_rejects_temperature_below_005_granularity(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    workspace = await _create_workspace(client, auth_headers)
    r = await client.post(
        "/api/v1/agents",
        headers=auth_headers,
        json={
            "name": "BadTemp",
            "system_prompt": "Reject bad temperature.",
            "workspace_id": workspace["id"],
            "llm_config": {"temperature": 0.07},
        },
    )

    assert r.status_code == 422


async def test_create_agent_persists_assistant_agent_tool(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    helper = await _create_agent(client, auth_headers, name="Helper")
    workspace = await _create_workspace(client, auth_headers)
    r = await client.post(
        "/api/v1/agents",
        headers=auth_headers,
        json={
            "name": "Caller",
            "system_prompt": "You can delegate.",
            "workspace_id": workspace["id"],
            "tool_config": {
                "assistant_agents": [{"agent_id": helper["id"], "enabled": True}],
                "tools": {"run_sub_agent": {"enabled": True}},
            },
        },
    )
    assert r.status_code == 201, r.text
    body = r.json()
    assert body["tool_config"]["assistant_agents"] == [
        {"agent_id": helper["id"], "enabled": True}
    ]


async def test_update_agent_rejects_self_assistant_tool(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    agent = await _create_agent(client, auth_headers, name="Selfish")
    r = await client.patch(
        f"/api/v1/agents/{agent['id']}",
        headers=auth_headers,
        json={"tool_config": {"assistant_agents": [{"agent_id": agent["id"], "enabled": True}]}},
    )
    assert r.status_code == 400


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


async def test_delete_agent_deletes_own_workspace_but_keeps_group_workspace(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    group_workspace = await _create_workspace(client, auth_headers)
    agent_workspace = await _create_workspace(client, auth_headers)
    agent_response = await client.post(
        "/api/v1/agents",
        headers=auth_headers,
        json={
            "name": "Scoped Delete",
            "system_prompt": "You have your own workspace.",
            "workspace_id": agent_workspace["id"],
        },
    )
    assert agent_response.status_code == 201, agent_response.text
    agent = agent_response.json()
    group_response = await client.post(
        "/api/v1/groups",
        headers=auth_headers,
        json={
            "name": "Keep Group Workspace",
            "workspace_id": group_workspace["id"],
            "initial_agents": [agent["id"]],
        },
    )
    assert group_response.status_code == 201, group_response.text

    response = await client.delete(f"/api/v1/agents/{agent['id']}", headers=auth_headers)
    assert response.status_code == 204, response.text

    own_workspace = await client.get(
        f"/api/v1/workspaces/{agent_workspace['id']}",
        headers=auth_headers,
    )
    assert own_workspace.status_code == 200, own_workspace.text
    assert own_workspace.json()["status"] == "deleted"

    still_group_workspace = await client.get(
        f"/api/v1/workspaces/{group_workspace['id']}",
        headers=auth_headers,
    )
    assert still_group_workspace.status_code == 200, still_group_workspace.text
    assert still_group_workspace.json()["status"] == "active"


async def test_delete_agent_keeps_workspace_used_by_another_active_agent(
    client: AsyncClient, auth_headers: dict[str, str]
) -> None:
    workspace = await _create_workspace(client, auth_headers)
    agents: list[JsonObject] = []
    for name in ["First", "Second"]:
        response = await client.post(
            "/api/v1/agents",
            headers=auth_headers,
            json={
                "name": name,
                "system_prompt": f"You are {name}.",
                "workspace_id": workspace["id"],
            },
        )
        assert response.status_code == 201, response.text
        agents.append(cast(JsonObject, response.json()))

    response = await client.delete(
        f"/api/v1/agents/{agents[0]['id']}",
        headers=auth_headers,
    )
    assert response.status_code == 204, response.text

    workspace_response = await client.get(
        f"/api/v1/workspaces/{workspace['id']}",
        headers=auth_headers,
    )
    assert workspace_response.status_code == 200, workspace_response.text
    assert workspace_response.json()["status"] == "active"

    surviving_agent = await client.get(
        f"/api/v1/agents/{agents[1]['id']}",
        headers=auth_headers,
    )
    assert surviving_agent.status_code == 200, surviving_agent.text
    assert surviving_agent.json()["workspace_id"] == workspace["id"]


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


async def test_invoke_uses_acp_runtime(
    client: AsyncClient,
    auth_headers: dict[str, str],
    monkeypatch: Any,
) -> None:
    workspace = await _create_workspace(client, auth_headers)
    r = await client.post(
        "/api/v1/agents",
        headers=auth_headers,
        json={
            "name": "ACP Invoke",
            "workspace_id": workspace["id"],
            "runtime_kind": "acp",
            "acp_runtime": {"command": sys.executable},
        },
    )
    assert r.status_code == 201, r.text
    agent = r.json()
    calls: list[str] = []

    async def _fake_run_acp_agent(_db: Any, **kwargs: Any) -> str:
        calls.append(str(kwargs["prompt"]))
        return "from acp"

    monkeypatch.setattr("app.api.v1.agents.run_acp_agent", _fake_run_acp_agent)
    r = await client.post(
        f"/api/v1/agents/{agent['id']}/invoke",
        headers=auth_headers,
        json={"message": "anything"},
    )

    assert r.status_code == 200, r.text
    assert r.json()["content"] == "from acp"
    assert "User request:\nanything" in calls[0]


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
