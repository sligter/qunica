from uuid import uuid4

from sqlalchemy.ext.asyncio import AsyncSession

from app.agents.builtin_tools import normalize_tool_config
from app.agents.context import build_agent_invocation_context
from app.models.agent import Agent
from app.models.group import Group
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
    group = Group(owner_id=user.id, name="Team", announcement="Be concise")
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
    assert "Repo Workspace" in prompt
    assert "enabled built-in tools: Read, Glob, Grep, AskUser" in prompt
    assert "# Skill: Reviewer" in prompt
    assert "version: 1.0.0" in prompt
    assert "Review the diff." in prompt
    assert "Runtime limits:" in prompt
    assert "file_mutations: 0" in prompt
