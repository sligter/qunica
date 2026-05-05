from uuid import uuid4

from sqlalchemy.ext.asyncio import AsyncSession

from app.agents.builtin_tools import AgentToolConfig, AgentToolSelection, normalize_tool_config
from app.agents.context import build_agent_invocation_context
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
    assert "Review the diff." in prompt
    assert "Runtime limits:" in prompt
    assert "file_mutations: 0" in prompt


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
                    "glob": AgentToolSelection(enabled=False),
                    "grep": AgentToolSelection(enabled=False),
                }
            )
        ),
    )
    db_session.add(agent)
    await db_session.flush()

    context = await build_agent_invocation_context(db_session, agent, user)

    assert context.executable_tools == ["Read"]
    assert "selected built-in tools: Read, Write, Edit, Bash" in context.system_prompt
    assert "executable built-in tools now: Read" in context.system_prompt
    assert "saved-only/planned selections: Write, Edit, Bash" in context.system_prompt
    assert "Do not claim you can create, write, edit, run code" in context.system_prompt


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
