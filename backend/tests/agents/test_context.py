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
from app.models.group_member import GroupMember
from app.models.skill import Skill
from app.models.system_settings import SystemSettings
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


async def test_group_context_lists_active_participants_and_marks_current_agent(
    db_session: AsyncSession,
) -> None:
    user = User(email=f"ctx-{uuid4().hex[:8]}@example.com", password_hash="x", name="Owner")
    teammate = User(
        email=f"ctx-member-{uuid4().hex[:8]}@example.com",
        password_hash="x",
        name="Reviewer",
    )
    db_session.add_all([user, teammate])
    await db_session.flush()
    group = Group(owner_id=user.id, name="Debate Room")
    current_agent = Agent(
        owner_id=user.id,
        name="Assistant",
        system_prompt="Base prompt",
    )
    other_agent = Agent(
        owner_id=user.id,
        name="Critic",
        system_prompt="Find issues",
    )
    removed_agent = Agent(
        owner_id=user.id,
        name="Removed",
        system_prompt="Should not appear",
    )
    deleted_agent = Agent(
        owner_id=user.id,
        name="Deleted",
        system_prompt="Should not appear",
        status="deleted",
    )
    db_session.add_all([group, current_agent, other_agent, removed_agent, deleted_agent])
    await db_session.flush()
    current_group_agent = GroupAgent(
        group_id=group.id,
        agent_id=current_agent.id,
        display_name="助手",
        role="participant",
    )
    db_session.add_all(
        [
            GroupMember(group_id=group.id, user_id=user.id, role="owner"),
            GroupMember(group_id=group.id, user_id=teammate.id, role="member"),
            current_group_agent,
            GroupAgent(group_id=group.id, agent_id=other_agent.id, display_name="找茬者"),
            GroupAgent(
                group_id=group.id,
                agent_id=removed_agent.id,
                display_name="Removed Agent",
                status="removed",
            ),
            GroupAgent(group_id=group.id, agent_id=deleted_agent.id, display_name="Deleted Agent"),
        ]
    )
    await db_session.flush()

    context = await build_agent_invocation_context(
        db_session,
        current_agent,
        user,
        group=group,
        group_agent=current_group_agent,
    )
    prompt = context.system_prompt

    assert "Group participants:" in prompt
    assert "@助手 (you, agent_name=Assistant" in prompt
    assert "@找茬者 (agent_name=Critic" in prompt
    assert "Use the listed @display names when addressing another agent" in prompt
    assert "Owner (owner)" in prompt
    assert "Reviewer (member)" in prompt
    assert "Removed Agent" not in prompt
    assert "Deleted Agent" not in prompt
    assert {participant.display_name for participant in context.group_agent_participants} == {
        "助手",
        "找茬者",
    }
    assert {participant.display_name for participant in context.group_human_participants} == {
        "Owner",
        "Reviewer",
    }


async def test_context_truthfully_reports_no_mounted_skills(
    db_session: AsyncSession,
) -> None:
    user = User(email=f"ctx-{uuid4().hex[:8]}@example.com", password_hash="x", name="Context User")
    db_session.add(user)
    await db_session.flush()
    agent = Agent(
        owner_id=user.id,
        name="Nova",
        system_prompt="Base prompt",
        tool_config=normalize_tool_config(None),
    )
    db_session.add(agent)
    await db_session.flush()

    context = await build_agent_invocation_context(db_session, agent, user)

    assert context.mounted_skills == []
    assert "Mounted skills:\n- none" in context.system_prompt
    assert "Mounted skills are selected on this agent and are independent of workspace files." in (
        context.system_prompt
    )


async def test_context_excludes_inactive_mounted_skill_ids(
    db_session: AsyncSession,
) -> None:
    user = User(email=f"ctx-{uuid4().hex[:8]}@example.com", password_hash="x", name="Context User")
    db_session.add(user)
    await db_session.flush()
    skill = Skill(
        owner_id=user.id,
        name="Deleted Skill",
        description="Should not be advertised",
        body_markdown="Secret stale instructions.",
        status="deleted",
    )
    db_session.add(skill)
    await db_session.flush()
    agent = Agent(
        owner_id=user.id,
        name="Nova",
        system_prompt="Base prompt",
        skill_ids=[str(skill.id)],
        tool_config=normalize_tool_config(None),
    )
    db_session.add(agent)
    await db_session.flush()

    context = await build_agent_invocation_context(db_session, agent, user)

    assert context.mounted_skills == []
    assert "Deleted Skill" not in context.system_prompt
    assert "Secret stale instructions." not in context.system_prompt
    assert "Mounted skills:\n- none" in context.system_prompt


async def test_context_excludes_other_owner_skill_ids_even_if_referenced(
    db_session: AsyncSession,
) -> None:
    owner = User(email=f"ctx-{uuid4().hex[:8]}@example.com", password_hash="x", name="Owner")
    other = User(
        email=f"ctx-other-{uuid4().hex[:8]}@example.com",
        password_hash="x",
        name="Other",
    )
    db_session.add_all([owner, other])
    await db_session.flush()
    other_skill = Skill(
        owner_id=other.id,
        name="Other Owner Skill",
        description="Should never be mounted",
        body_markdown="Cross-owner instructions.",
        status="active",
    )
    db_session.add(other_skill)
    await db_session.flush()
    agent = Agent(
        owner_id=owner.id,
        name="Nova",
        system_prompt="Base prompt",
        skill_ids=[str(other_skill.id)],
        tool_config=normalize_tool_config(None),
    )
    db_session.add(agent)
    await db_session.flush()

    context = await build_agent_invocation_context(db_session, agent, owner)

    assert context.mounted_skills == []
    assert "Other Owner Skill" not in context.system_prompt
    assert "Cross-owner instructions." not in context.system_prompt
    assert "Mounted skills:\n- none" in context.system_prompt


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
    assert "generous default timeout" in context.system_prompt
    assert "can be overridden with timeout_seconds" in context.system_prompt


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
    all_enabled = {tool.id: AgentToolSelection(enabled=True) for tool in list_builtin_tools()}
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
    assert "SETUP_REQUIRED" in execute_workspace_tool(tools, "WebSearch", {"query": "current news"})
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
    monkeypatch.setattr(
        "app.agents.workspace_tools.settings.tavily_search_url", "https://tavily.test/search"
    )
    monkeypatch.setattr("app.agents.workspace_tools.settings.playwright_search_url", "")
    context = await build_agent_invocation_context(db_session, agent, user)
    tools = build_workspace_tools(context)

    result = json.loads(execute_workspace_tool(tools, "WebSearch", {"query": "latest"}))

    assert result["status"] == "COMPLETED"
    assert result["answer"] == "real provider answer"
    assert result["results"][0]["content"] == "provider snippet"
    assert requests[0].url == "https://tavily.test/search"


async def test_web_search_prefers_user_tavily_settings_and_allows_configured_limit(
    db_session: AsyncSession,
    monkeypatch: Any,
) -> None:
    user = User(email=f"ctx-{uuid4().hex[:8]}@example.com", password_hash="x", name="Context User")
    db_session.add(user)
    await db_session.flush()
    db_session.add(
        SystemSettings(
            owner_id=user.id,
            web_search_provider="tavily",
            tavily_api_key="user-key",
            tavily_search_url="https://user-tavily.test/search",
            tavily_max_results=10,
            tavily_search_depth="advanced",
            tavily_include_answer=False,
            tavily_include_raw_content=True,
        )
    )
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
        return httpx.Response(200, json={"results": []}, request=request)

    class MockClient(httpx.Client):
        def __init__(self, *args: Any, **kwargs: Any) -> None:
            _ = (args, kwargs)
            super().__init__(transport=httpx.MockTransport(_handler))

    monkeypatch.setattr(httpx, "Client", MockClient)
    monkeypatch.setattr("app.agents.workspace_tools.settings.tavily_api_key", "env-key")
    monkeypatch.setattr(
        "app.agents.workspace_tools.settings.tavily_search_url", "https://env-tavily.test/search"
    )
    context = await build_agent_invocation_context(db_session, agent, user)
    tools = build_workspace_tools(context)

    result = json.loads(
        execute_workspace_tool(tools, "WebSearch", {"query": "latest", "max_results": 10})
    )

    assert result["status"] == "COMPLETED"
    assert requests[0].url == "https://user-tavily.test/search"
    payload = json.loads(requests[0].content.decode())
    assert payload == {
        "api_key": "user-key",
        "query": "latest",
        "max_results": 10,
        "include_answer": False,
        "include_raw_content": True,
        "search_depth": "advanced",
    }


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
    monkeypatch.setattr(
        "app.agents.workspace_tools.settings.playwright_search_url", "https://search.test/query"
    )
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
    assert "WORKSPACE_REQUIRED" in execute_workspace_tool(tools, "Read", {"file_path": "README.md"})
    assert "WORKSPACE_REQUIRED" in execute_workspace_tool(tools, "Bash", {"command": "pwd"})


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
