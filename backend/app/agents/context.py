"""Shared context assembly for direct and group agent invocations.

This module builds prompt/context metadata. Provider-native tools listed as
executable may run in the runtime from the same resolved context with bounded
workspace/network safeguards; this module itself only renders the contract.
"""

from dataclasses import dataclass
from typing import Any
from uuid import UUID

from langchain_core.messages import BaseMessage, SystemMessage
from sqlalchemy.ext.asyncio import AsyncSession

from app.agents.builtin_tools import (
    executable_tool_names,
    saved_only_tool_names,
    selected_tool_names,
)
from app.models.agent import Agent
from app.models.group import Group
from app.models.group_agent import GroupAgent
from app.models.skill import Skill
from app.models.user import User
from app.models.workspace import Workspace
from app.services import skill_service, workspace_service

DEFAULT_RUNTIME_LIMITS: dict[str, int] = {
    "context_history_messages": 20,
    "tool_iterations": 5,
    "file_mutation_bytes": 1_000_000,
    "bash_timeout_seconds": 10,
    "fetch_bytes": 500_000,
}


@dataclass(frozen=True, slots=True)
class AgentInvocationContext:
    """Prompt context shared by direct invoke and group message flows."""

    system_prompt: str
    workspace: Workspace | None
    enabled_tools: list[str]
    executable_tools: list[str]
    saved_only_tools: list[str]
    setup_required_tools: list[str]
    mounted_skills: list[Skill]
    runtime_limits: dict[str, int]
    workspace_source: str

    def to_system_message(self) -> SystemMessage:
        return SystemMessage(content=self.system_prompt)


async def build_agent_invocation_context(
    db: AsyncSession,
    agent: Agent,
    user: User,
    *,
    group: Group | None = None,
    group_agent: GroupAgent | None = None,
    runtime_limits: dict[str, int] | None = None,
) -> AgentInvocationContext:
    """Build the shared prompt context for an agent invocation.

    Includes the agent prompt, optional group announcement/context, workspace
    metadata, enabled built-in tools, mounted skill metadata/instructions, and
    explicit runtime limits. Provider-native tools listed as executable can run
    from this resolved context with bounded safeguards.
    """

    limits = dict(DEFAULT_RUNTIME_LIMITS)
    if runtime_limits is not None:
        limits.update(runtime_limits)

    workspace = None
    workspace_source = "none"
    share_group_workspace = bool(
        group_agent is not None
        and (group_agent.context_scope or {}).get("share_group_workspace") is True
    )
    if share_group_workspace and group is not None and group.workspace_id is not None:
        workspace = await workspace_service.get_active_workspace(db, group.workspace_id, user)
        workspace_source = "group"
    elif agent.workspace_id is not None:
        workspace = await workspace_service.get_active_workspace(db, agent.workspace_id, user)
        workspace_source = "agent"

    skills = await _mounted_skills(db, agent)
    selected_tools = selected_tool_names(agent.tool_config)
    executable_tools = executable_tool_names(agent.tool_config)
    saved_only_tools = saved_only_tool_names(agent.tool_config)
    setup_required_tools = [
        name
        for name in executable_tools
        if name
        in {
            "AskUser",
            "WebSearch",
            "RunSubAgent",
            "GenerateImage",
            "GenerateVideo",
            "TodoWrite",
            "ExitPlanMode",
        }
    ]
    system_prompt = _render_system_prompt(
        agent=agent,
        group=group,
        workspace=workspace,
        workspace_source=workspace_source,
        selected_tools=selected_tools,
        executable_tools=executable_tools,
        saved_only_tools=saved_only_tools,
        setup_required_tools=setup_required_tools,
        skills=skills,
        runtime_limits=limits,
    )
    return AgentInvocationContext(
        system_prompt=system_prompt,
        workspace=workspace,
        enabled_tools=selected_tools,
        executable_tools=executable_tools,
        saved_only_tools=saved_only_tools,
        setup_required_tools=setup_required_tools,
        mounted_skills=skills,
        runtime_limits=limits,
        workspace_source=workspace_source,
    )


async def build_agent_system_message(
    db: AsyncSession,
    agent: Agent,
    user: User,
    *,
    group: Group | None = None,
    group_agent: GroupAgent | None = None,
    runtime_limits: dict[str, int] | None = None,
) -> BaseMessage:
    """Convenience wrapper for callers that only need a LangChain message."""

    context = await build_agent_invocation_context(
        db,
        agent,
        user,
        group=group,
        group_agent=group_agent,
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
    workspace_source: str,
    selected_tools: list[str],
    executable_tools: list[str],
    saved_only_tools: list[str],
    setup_required_tools: list[str],
    skills: list[Skill],
    runtime_limits: dict[str, int],
) -> str:
    parts: list[str] = [agent.system_prompt]

    if group is not None:
        parts.append(_render_group_context(group))
    parts.append(
        _render_workspace_context(
            workspace,
            workspace_source,
            selected_tools,
            executable_tools,
            saved_only_tools,
            setup_required_tools,
        )
    )
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
    lines.append(f"- proactive_mode: {group.proactive_mode}")
    lines.append(f"- proactive_reply_multiplier: {group.proactive_reply_multiplier}")
    if group.proactive_mode:
        lines.append(
            "- proactive participation: You are participating in a group chat. "
            "After reading the conversation, decide whether you have anything "
            "substantive to add. If you do, reply normally. If you would rather "
            "stay silent, reply with exactly the single token `<SILENT>` (no other "
            "characters, no punctuation, no whitespace before or after). The system "
            "will skip your turn and not persist a message."
        )
    lines.append(f"- allow_agent_free_mention: {group.allow_agent_free_mention}")
    return "\n".join(lines)


def _render_workspace_context(
    workspace: Workspace | None,
    workspace_source: str,
    selected_tools: list[str],
    executable_tools: list[str],
    saved_only_tools: list[str],
    setup_required_tools: list[str],
) -> str:
    tools = ", ".join(selected_tools) if selected_tools else "none"
    executable = ", ".join(executable_tools) if executable_tools else "none"
    saved_only = ", ".join(saved_only_tools) if saved_only_tools else "none"
    setup_required = ", ".join(setup_required_tools) if setup_required_tools else "none"
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
        f"- source: {workspace_source}\n"
        f"- name: {name}\n"
        f"- backend_type: {backend_type}\n"
        f"- location: {location or 'not configured'}\n"
        f"- selected built-in tools: {tools}\n"
        f"- executable built-in tools now: {executable}\n"
        f"- executable tools that may return setup/input-required results: {setup_required}\n"
        f"- saved-only/planned selections: {saved_only}\n"
        "Runtime tool execution: only provider-native tool calls listed as executable above "
        "may execute with bounded safeguards. Literal XML-like tool markup in text is not "
        "executed. Workspace file and shell tools are rooted at the resolved local workspace "
        "and reject absolute paths, traversal, and root escapes. Bash commands run in the "
        "workspace with timeout/output limits and destructive command guards. Fetch is "
        "limited to bounded text http/https GET requests. WebSearch uses configured Tavily "
        "credentials or a configured Playwright-backed search service when available and "
        "otherwise returns a controlled setup-required tool result. AskUser returns a "
        "non-blocking WAITING_FOR_USER result. Media generation, "
        "sub-agent orchestration, plan-exit, and transient todo tools are executable bounded "
        "tool calls; if no provider or persistence contract is configured they return "
        "truthful controlled tool results instead of pretending work completed. SkillManager "
        "can inspect mounted skill metadata and records activation intent only; it does not "
        "load arbitrary code."
    )


def _render_runtime_limits(runtime_limits: dict[str, int]) -> str:
    lines = ["Runtime limits:"]
    for key in sorted(runtime_limits):
        lines.append(f"- {key}: {runtime_limits[key]}")
    return "\n".join(lines)


def _render_skills(skills: list[Skill]) -> str:
    rendered = [
        "Mounted skills:",
        "Full skill instructions are loaded only through SkillManager inspect/activate "
        "runtime tool calls; initial context lists metadata only.",
    ]
    for skill in skills:
        metadata = skill.metadata_ or {}
        rendered.append(_render_skill_metadata(skill, metadata))
    return "\n\n".join(rendered)


def _render_skill_metadata(skill: Skill, metadata: dict[str, Any]) -> str:
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
    return "\n".join(lines)
