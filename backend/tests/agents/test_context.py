import json
from typing import Any
from uuid import uuid4

import httpx
from sqlalchemy.ext.asyncio import AsyncSession

from app.agents.builtin_tools import (
    AgentToolConfig,
    AgentToolSelection,
    list_builtin_tools,
    normalize_tool_config,
)
from app.agents.context import build_agent_invocation_context
from app.agents.workspace_tools import build_workspace_tools, execute_workspace_tool
from app.models.agent import Agent
from app.models.group import Group
from app.models.group_agent import GroupAgent
from app.models.skill import Skill
from app.models.user import User
from app.models.workspace import Workspace


async def test_shared_context_includes_workspace_tools_skills_and_limits(
    db_session: AsyncSession,
) -> None:
    user = User(email=f"ctx-{uuid4().hex[:8]}@example.com", password_hash="x", name="Context User")
    db_session.add(user)
    await db_session.flush()
    workspace = Workspace(
        owner_id=user.id,
        name="Repo Workspace",
        backend_type="local",
        local_path="/repo",
    )
    db_session.add(workspace)
    skill = Skill(
        owner_id=user.id,
        name="Reviewer",
        description="Review changes",
        body_markdown="Review the diff.",
        metadata_={"version": "1.0.0", "tools": ["read", "grep"]},
        source="imported",
    )
    db_session.add(skill)
    await db_session.flush()
    group = Group(
        owner_id=user.id,
        workspace_id=workspace.id,
        name="Team",
        announcement="Be concise",
    )
    db_session.add(group)
    await db_session.flush()
    agent = Agent(
        owner_id=user.id,
        name="Nova",
        system_prompt="Base prompt",
        workspace_id=workspace.id,
        skill_ids=[str(skill.id)],
        tool_config=normalize_tool_config(None),
    )
    db_session.add(agent)
    await db_session.flush()

    context = await build_agent_invocation_context(db_session, agent, user, group=group)
    prompt = context.system_prompt

    assert "Base prompt" in prompt
    assert "Group context:" in prompt
    assert "Be concise" in prompt
    assert "source: agent" in prompt
    assert "Repo Workspace" in prompt
    assert "selected built-in tools: Read, Glob, Grep" in prompt
    assert "executable built-in tools now: Read, Glob, Grep" in prompt
    assert "saved-only/planned selections: none" in prompt
    assert "# Skill: Reviewer" in prompt
    assert "version: 1.0.0" in prompt
    assert "Full skill instructions are loaded only through SkillManager" in prompt
    assert "Review the diff." not in prompt
    assert "Runtime limits:" in prompt
    assert "file_mutation_bytes: 1000000" in prompt


async def test_context_distinguishes_executable_tools_from_saved_only_tools(
    db_session: AsyncSession,
) -> None:
    user = User(email=f"ctx-{uuid4().hex[:8]}@example.com", password_hash="x", name="Context User")
    db_session.add(user)
    await db_session.flush()
    workspace = Workspace(
        owner_id=user.id,
        name="Repo Workspace",
        backend_type="local",
        local_path="/repo",
    )
    db_session.add(workspace)
    agent = Agent(
        owner_id=user.id,
        name="Nova",
        system_prompt="Base prompt",
        workspace_id=workspace.id,
        tool_config=normalize_tool_config(
            AgentToolConfig(
                tools={
                    "read": AgentToolSelection(enabled=True),
                    "write": AgentToolSelection(enabled=True),
                    "edit": AgentToolSelection(enabled=True),
                    "bash": AgentToolSelection(enabled=True),
                    "web_search": AgentToolSelection(enabled=True),
                    "ask_user": AgentToolSelection(enabled=True),
                    "glob": AgentToolSelection(enabled=False),
                    "grep": AgentToolSelection(enabled=False),
                }
            )
        ),
    )
    db_session.add(agent)
    await db_session.flush()

    context = await build_agent_invocation_context(db_session, agent, user)

    assert context.executable_tools == ["Read", "Write", "Edit", "Bash", "WebSearch", "AskUser"]
    assert (
        "selected built-in tools: Read, Write, Edit, Bash, WebSearch, AskUser"
        in context.system_prompt
    )
    assert (
        "executable built-in tools now: Read, Write, Edit, Bash, WebSearch, AskUser"
        in context.system_prompt
    )
    assert "saved-only/planned selections: none" in context.system_prompt
    assert "saved-only/planned selections: none" in context.system_prompt
    assert "WebSearch uses configured Tavily" in context.system_prompt
    assert "AskUser returns a non-blocking WAITING_FOR_USER result" in context.system_prompt
    assert "Bash commands run in the workspace" in context.system_prompt


async def test_every_catalog_builtin_selected_is_executable_in_context(
    db_session: AsyncSession,
) -> None:
    user = User(email=f"ctx-{uuid4().hex[:8]}@example.com", password_hash="x", name="Context User")
    db_session.add(user)
    await db_session.flush()
    workspace = Workspace(
        owner_id=user.id,
        name="Repo Workspace",
        backend_type="local",
        local_path="/repo",
    )
    db_session.add(workspace)
    all_enabled = {
        tool.id: AgentToolSelection(enabled=True)
        for tool in list_builtin_tools()
    }
    agent = Agent(
        owner_id=user.id,
        name="Nova",
        system_prompt="Base prompt",
        workspace_id=workspace.id,
        tool_config=normalize_tool_config(AgentToolConfig(tools=all_enabled)),
    )
    db_session.add(agent)
    await db_session.flush()

    context = await build_agent_invocation_context(db_session, agent, user)

    assert context.executable_tools == [tool.name for tool in list_builtin_tools()]
    assert context.saved_only_tools == []
    assert "saved-only/planned selections: none" in context.system_prompt


async def test_skill_manager_lists_metadata_then_loads_instructions_on_inspect(
    db_session: AsyncSession,
) -> None:
    user = User(email=f"ctx-{uuid4().hex[:8]}@example.com", password_hash="x", name="Context User")
    db_session.add(user)
    await db_session.flush()
    skill = Skill(
        owner_id=user.id,
        name="DeckBuilder",
        description="Build decks",
        body_markdown="Full PPT instructions.",
        metadata_={"version": "2.0.0"},
        source="imported",
    )
    db_session.add(skill)
    await db_session.flush()
    agent = Agent(
        owner_id=user.id,
        name="Nova",
        system_prompt="Base prompt",
        skill_ids=[str(skill.id)],
        tool_config=normalize_tool_config(
            AgentToolConfig(tools={"skill_manager": AgentToolSelection(enabled=True)})
        ),
    )
    db_session.add(agent)
    await db_session.flush()

    context = await build_agent_invocation_context(db_session, agent, user)
    tools = build_workspace_tools(context)

    listed = execute_workspace_tool(tools, "SkillManager", {"action": "list"})
    inspected = execute_workspace_tool(
        tools, "SkillManager", {"action": "inspect", "skill_name": "DeckBuilder"}
    )
    assert "Full PPT instructions." not in context.system_prompt
    assert "Full PPT instructions." not in listed
    assert "Full PPT instructions." in inspected


async def test_controlled_runtime_tools_bind_and_execute_without_provider_config(
    db_session: AsyncSession,
) -> None:
    user = User(email=f"ctx-{uuid4().hex[:8]}@example.com", password_hash="x", name="Context User")
    db_session.add(user)
    await db_session.flush()
    agent = Agent(
        owner_id=user.id,
        name="Nova",
        system_prompt="Base prompt",
        tool_config=normalize_tool_config(
            AgentToolConfig(
                tools={
                    "web_search": AgentToolSelection(enabled=True),
                    "ask_user": AgentToolSelection(enabled=True),
                    "run_sub_agent": AgentToolSelection(enabled=True),
                    "generate_image": AgentToolSelection(enabled=True),
                    "generate_video": AgentToolSelection(enabled=True),
                    "skill_manager": AgentToolSelection(enabled=True),
                    "todo_write": AgentToolSelection(enabled=True),
                    "exit_plan_mode": AgentToolSelection(enabled=True),
                    "read": AgentToolSelection(enabled=False),
                    "glob": AgentToolSelection(enabled=False),
                    "grep": AgentToolSelection(enabled=False),
                }
            )
        ),
    )
    db_session.add(agent)
    await db_session.flush()

    context = await build_agent_invocation_context(db_session, agent, user)
    tools = build_workspace_tools(context)

    assert set(tools) == {
        "AskUser",
        "WebSearch",
        "RunSubAgent",
        "GenerateImage",
        "GenerateVideo",
        "SkillManager",
        "TodoWrite",
        "ExitPlanMode",
    }
    assert "WAITING_FOR_USER" in execute_workspace_tool(
        tools, "AskUser", {"question": "Please provide the draft."}
    )
    assert "SETUP_REQUIRED" in execute_workspace_tool(
        tools, "WebSearch", {"query": "current news"}
    )
    assert "SETUP_REQUIRED" in execute_workspace_tool(
        tools, "GenerateImage", {"prompt": "a slide hero"}
    )
    assert "COMPLETED" in execute_workspace_tool(tools, "TodoWrite", {"todos": ["draft"]})
    assert "APPROVAL_REQUIRED" in execute_workspace_tool(
        tools, "ExitPlanMode", {"plan": "Make the deck."}
    )
    assert "unavailable" in execute_workspace_tool(tools, "MadeUp", {})


async def test_web_search_uses_configured_tavily_provider(
    db_session: AsyncSession,
    monkeypatch: Any,
) -> None:
    user = User(email=f"ctx-{uuid4().hex[:8]}@example.com", password_hash="x", name="Context User")
    db_session.add(user)
    await db_session.flush()
    agent = Agent(
        owner_id=user.id,
        name="Nova",
        system_prompt="Base prompt",
        tool_config=normalize_tool_config(
            AgentToolConfig(tools={"web_search": AgentToolSelection(enabled=True)})
        ),
    )
    db_session.add(agent)
    await db_session.flush()

    requests: list[httpx.Request] = []

    def _handler(request: httpx.Request) -> httpx.Response:
        requests.append(request)
        return httpx.Response(
            200,
            json={
                "answer": "real provider answer",
                "results": [
                    {
                        "title": "Result",
                        "url": "https://example.test/result",
                        "content": "provider snippet",
                    }
                ],
            },
            request=request,
        )

    class MockClient(httpx.Client):
        def __init__(self, *args: Any, **kwargs: Any) -> None:
            _ = (args, kwargs)
            super().__init__(transport=httpx.MockTransport(_handler))

    monkeypatch.setattr(httpx, "Client", MockClient)
    monkeypatch.setattr("app.agents.workspace_tools.settings.tavily_api_key", "test-key")
    monkeypatch.setattr("app.agents.workspace_tools.settings.tavily_search_url", "https://tavily.test/search")
    monkeypatch.setattr("app.agents.workspace_tools.settings.playwright_search_url", "")
    context = await build_agent_invocation_context(db_session, agent, user)
    tools = build_workspace_tools(context)

    result = json.loads(execute_workspace_tool(tools, "WebSearch", {"query": "latest"}))

    assert result["status"] == "COMPLETED"
    assert result["answer"] == "real provider answer"
    assert result["results"][0]["content"] == "provider snippet"
    assert requests[0].url == "https://tavily.test/search"


async def test_web_search_uses_configured_playwright_provider(
    db_session: AsyncSession,
    monkeypatch: Any,
) -> None:
    user = User(email=f"ctx-{uuid4().hex[:8]}@example.com", password_hash="x", name="Context User")
    db_session.add(user)
    await db_session.flush()
    agent = Agent(
        owner_id=user.id,
        name="Nova",
        system_prompt="Base prompt",
        tool_config=normalize_tool_config(
            AgentToolConfig(tools={"web_search": AgentToolSelection(enabled=True)})
        ),
    )
    db_session.add(agent)
    await db_session.flush()

    requests: list[httpx.Request] = []

    def _handler(request: httpx.Request) -> httpx.Response:
        requests.append(request)
        return httpx.Response(200, text="playwright result text", request=request)

    class MockClient(httpx.Client):
        def __init__(self, *args: Any, **kwargs: Any) -> None:
            _ = (args, kwargs)
            super().__init__(transport=httpx.MockTransport(_handler), follow_redirects=True)

    monkeypatch.setattr(httpx, "Client", MockClient)
    monkeypatch.setattr("app.agents.workspace_tools.settings.tavily_api_key", "")
    monkeypatch.setattr("app.agents.workspace_tools.settings.playwright_search_url", "https://search.test/query")
    context = await build_agent_invocation_context(db_session, agent, user)
    tools = build_workspace_tools(context)

    result = json.loads(execute_workspace_tool(tools, "WebSearch", {"query": "latest"}))

    assert result["status"] == "COMPLETED"
    assert result["provider"] == "playwright"
    assert result["content"] == "playwright result text"
    assert requests[0].url == "https://search.test/query?q=latest&max_results=5"


async def test_every_catalog_builtin_binds_without_workspace(
    db_session: AsyncSession,
) -> None:
    user = User(email=f"ctx-{uuid4().hex[:8]}@example.com", password_hash="x", name="Context User")
    db_session.add(user)
    await db_session.flush()
    all_enabled = {tool.id: AgentToolSelection(enabled=True) for tool in list_builtin_tools()}
    agent = Agent(
        owner_id=user.id,
        name="Nova",
        system_prompt="Base prompt",
        tool_config=normalize_tool_config(AgentToolConfig(tools=all_enabled)),
    )
    db_session.add(agent)
    await db_session.flush()

    context = await build_agent_invocation_context(db_session, agent, user)
    tools = build_workspace_tools(context)

    assert set(tools) == {tool.name for tool in list_builtin_tools()}
    assert "WORKSPACE_REQUIRED" in execute_workspace_tool(
        tools, "Read", {"file_path": "README.md"}
    )
    assert "WORKSPACE_REQUIRED" in execute_workspace_tool(
        tools, "Bash", {"command": "pwd"}
    )


async def test_group_workspace_sharing_switches_context_source(
    db_session: AsyncSession,
) -> None:
    user = User(email=f"ctx-{uuid4().hex[:8]}@example.com", password_hash="x", name="Context User")
    db_session.add(user)
    await db_session.flush()
    agent_workspace = Workspace(
        owner_id=user.id,
        name="Agent Workspace",
        backend_type="local",
        local_path="/agent",
    )
    group_workspace = Workspace(
        owner_id=user.id,
        name="Group Workspace",
        backend_type="local",
        local_path="/group",
    )
    db_session.add_all([agent_workspace, group_workspace])
    await db_session.flush()
    agent = Agent(
        owner_id=user.id,
        name="Nova",
        system_prompt="Base prompt",
        workspace_id=agent_workspace.id,
    )
    group = Group(owner_id=user.id, workspace_id=group_workspace.id, name="Team")
    db_session.add_all([agent, group])
    await db_session.flush()
    group_agent = GroupAgent(group_id=group.id, agent_id=agent.id)
    db_session.add(group_agent)
    await db_session.flush()

    default_context = await build_agent_invocation_context(
        db_session, agent, user, group=group, group_agent=group_agent
    )
    assert default_context.workspace_source == "agent"
    assert "Agent Workspace" in default_context.system_prompt

    group_agent.context_scope = {"share_group_workspace": True}
    shared_context = await build_agent_invocation_context(
        db_session, agent, user, group=group, group_agent=group_agent
    )
    assert shared_context.workspace_source == "group"
    assert "Group Workspace" in shared_context.system_prompt
    assert "source: group" in shared_context.system_prompt
