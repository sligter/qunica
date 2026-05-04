"""Shared context assembly for direct and group agent invocations.

This module only builds prompt/context metadata. It does not execute built-in
workspace tools, mutate files, or grant runtime permissions.
"""

from dataclasses import dataclass
from typing import Any
from uuid import UUID

from langchain_core.messages import BaseMessage, SystemMessage
from sqlalchemy.ext.asyncio import AsyncSession

from app.agents.builtin_tools import enabled_tool_names
from app.models.agent import Agent
from app.models.group import Group
from app.models.skill import Skill
from app.models.user import User
from app.models.workspace import Workspace
from app.services import skill_service, workspace_service

DEFAULT_RUNTIME_LIMITS: dict[str, int] = {
    "context_history_messages": 20,
    "tool_execution": 0,
    "file_mutations": 0,
}


@dataclass(frozen=True, slots=True)
class AgentInvocationContext:
    """Prompt context shared by direct invoke and group message flows."""

    system_prompt: str
    workspace: Workspace | None
    enabled_tools: list[str]
    mounted_skills: list[Skill]
    runtime_limits: dict[str, int]

    def to_system_message(self) -> SystemMessage:
        return SystemMessage(content=self.system_prompt)


async def build_agent_invocation_context(
    db: AsyncSession,
    agent: Agent,
    user: User,
    *,
    group: Group | None = None,
    runtime_limits: dict[str, int] | None = None,
) -> AgentInvocationContext:
    """Build the shared prompt context for an agent invocation.

    Includes the agent prompt, optional group announcement/context, workspace
    metadata, enabled built-in tools, mounted skill metadata/instructions, and
    explicit runtime limits. Built-in tools are represented as prompt metadata
    only; no risky tool execution is implemented here.
    """

    limits = dict(DEFAULT_RUNTIME_LIMITS)
    if runtime_limits is not None:
        limits.update(runtime_limits)

    workspace = None
    if agent.workspace_id is not None:
        workspace = await workspace_service.get_active_workspace(db, agent.workspace_id, user)

    skills = await _mounted_skills(db, agent)
    system_prompt = _render_system_prompt(
        agent=agent,
        group=group,
        workspace=workspace,
        enabled_tools=enabled_tool_names(agent.tool_config),
        skills=skills,
        runtime_limits=limits,
    )
    return AgentInvocationContext(
        system_prompt=system_prompt,
        workspace=workspace,
        enabled_tools=enabled_tool_names(agent.tool_config),
        mounted_skills=skills,
        runtime_limits=limits,
    )


async def build_agent_system_message(
    db: AsyncSession,
    agent: Agent,
    user: User,
    *,
    group: Group | None = None,
    runtime_limits: dict[str, int] | None = None,
) -> BaseMessage:
    """Convenience wrapper for callers that only need a LangChain message."""

    context = await build_agent_invocation_context(
        db,
        agent,
        user,
        group=group,
        runtime_limits=runtime_limits,
    )
    return context.to_system_message()


async def _mounted_skills(db: AsyncSession, agent: Agent) -> list[Skill]:
    if not agent.skill_ids:
        return []
    skill_uuids = [UUID(s) if isinstance(s, str) else s for s in agent.skill_ids]
    return await skill_service.list_by_ids(db, skill_uuids)


def _render_system_prompt(
    *,
    agent: Agent,
    group: Group | None,
    workspace: Workspace | None,
    enabled_tools: list[str],
    skills: list[Skill],
    runtime_limits: dict[str, int],
) -> str:
    parts: list[str] = [agent.system_prompt]

    if group is not None:
        parts.append(_render_group_context(group))
    parts.append(_render_workspace_context(workspace, enabled_tools))
    parts.append(_render_runtime_limits(runtime_limits))
    if skills:
        parts.append(_render_skills(skills))

    return "\n\n".join(part for part in parts if part)


def _render_group_context(group: Group) -> str:
    lines = ["Group context:", f"- name: {group.name}"]
    if group.description:
        lines.append(f"- description: {group.description}")
    if group.announcement:
        lines.append(f"- announcement: {group.announcement}")
    lines.append(f"- free_speech: {group.free_speech}")
    lines.append(f"- allow_agent_free_mention: {group.allow_agent_free_mention}")
    return "\n".join(lines)


def _render_workspace_context(workspace: Workspace | None, enabled_tools: list[str]) -> str:
    tools = ", ".join(enabled_tools) if enabled_tools else "none"
    location: str | None
    if workspace is None:
        location = "not configured"
        backend_type = "none"
        name = "not configured"
    else:
        location = (
            workspace.local_path
            if workspace.backend_type == "local"
            else workspace.sandbox_ref
        )
        backend_type = workspace.backend_type
        name = workspace.name
    return (
        "Agent workspace context:\n"
        f"- name: {name}\n"
        f"- backend_type: {backend_type}\n"
        f"- location: {location or 'not configured'}\n"
        f"- enabled built-in tools: {tools}\n"
        "Built-in tools are declared for context only. This runtime does not execute "
        "bash, write, edit, or other risky tools."
    )


def _render_runtime_limits(runtime_limits: dict[str, int]) -> str:
    lines = ["Runtime limits:"]
    for key in sorted(runtime_limits):
        lines.append(f"- {key}: {runtime_limits[key]}")
    return "\n".join(lines)


def _render_skills(skills: list[Skill]) -> str:
    rendered = ["Mounted skills:"]
    for skill in skills:
        metadata = skill.metadata_ or {}
        rendered.append(_render_skill(skill, metadata))
    return "\n\n".join(rendered)


def _render_skill(skill: Skill, metadata: dict[str, Any]) -> str:
    lines = [f"# Skill: {skill.name}"]
    if skill.description:
        lines.append(f"Description: {skill.description}")
    for key in (
        "version",
        "author",
        "license",
        "icon",
        "activation",
        "trigger",
        "tools",
        "capabilities",
    ):
        value = metadata.get(key)
        if value is not None:
            lines.append(f"{key}: {value}")
    lines.append(skill.body_markdown)
    return "\n".join(lines)
