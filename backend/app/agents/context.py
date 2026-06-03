"""Shared context assembly for direct and group agent invocations.

This module builds prompt/context metadata. Provider-native tools listed as
executable may run in the runtime from the same resolved context with bounded
workspace/network safeguards; this module itself only renders the contract.
"""

from dataclasses import dataclass
from typing import Any
from uuid import UUID

from langchain_core.messages import BaseMessage, SystemMessage
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.agents.builtin_tools import (
    AgentToolConfig,
    executable_tool_names,
    normalize_tool_config,
    saved_only_tool_names,
    selected_tool_names,
)
from app.models.agent import Agent
from app.models.group import Group
from app.models.group_agent import GroupAgent
from app.models.group_member import GroupMember
from app.models.skill import Skill
from app.models.user import User
from app.models.workspace import Workspace
from app.services import skill_service, system_settings_service, workspace_service
from app.services.system_settings_service import TavilySearchConfig

DEFAULT_RUNTIME_LIMITS: dict[str, int] = {
    "context_history_messages": 20,
    "tool_iterations": 5,
    "file_mutation_bytes": 1_000_000,
    "bash_default_timeout_seconds": 600,
    "bash_max_timeout_seconds": 3_600,
    "fetch_bytes": 500_000,
}


@dataclass(frozen=True, slots=True)
class GroupAgentParticipant:
    """Active agent visible to the invoked agent in a group chat."""

    agent_id: UUID
    display_name: str
    agent_name: str
    role: str | None
    topology_role: str | None
    response_mode: str
    is_self: bool


@dataclass(frozen=True, slots=True)
class GroupHumanParticipant:
    """Active human member visible to the invoked agent in a group chat."""

    user_id: UUID
    display_name: str
    role: str


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
    assistant_agents: list[Agent]
    group_agent_participants: list[GroupAgentParticipant]
    group_human_participants: list[GroupHumanParticipant]
    runtime_limits: dict[str, int]
    workspace_source: str
    tavily_search: TavilySearchConfig | None

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

    system_settings = await system_settings_service.get_or_create(db, user)
    tavily_search = system_settings_service.tavily_config_from_settings(system_settings)
    skills = await _mounted_skills(db, agent, user)
    assistant_agents = await _assistant_agents(db, agent)
    group_agent_participants: list[GroupAgentParticipant] = []
    group_human_participants: list[GroupHumanParticipant] = []
    if group is not None:
        group_agent_participants = await _group_agent_participants(db, group.id, agent.id)
        group_human_participants = await _group_human_participants(db, group.id)
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
        assistant_agents=assistant_agents,
        group_agent_participants=group_agent_participants,
        group_human_participants=group_human_participants,
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
        assistant_agents=assistant_agents,
        group_agent_participants=group_agent_participants,
        group_human_participants=group_human_participants,
        runtime_limits=limits,
        workspace_source=workspace_source,
        tavily_search=tavily_search,
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


async def _mounted_skills(db: AsyncSession, agent: Agent, user: User) -> list[Skill]:
    if not agent.skill_ids:
        return []
    skill_uuids = [UUID(s) if isinstance(s, str) else s for s in agent.skill_ids]
    return await skill_service.list_by_ids(db, skill_uuids, owner=user)


async def _assistant_agents(db: AsyncSession, agent: Agent) -> list[Agent]:
    config = AgentToolConfig.model_validate(agent.tool_config or normalize_tool_config(None))
    assistant_ids = [
        selection.agent_id
        for selection in config.assistant_agents
        if selection.enabled and selection.agent_id != agent.id
    ]
    if not assistant_ids:
        return []
    rows = await db.scalars(
        select(Agent)
        .where(
            Agent.id.in_(assistant_ids),
            Agent.owner_id == agent.owner_id,
            Agent.status == "active",
        )
    )
    agents_by_id = {assistant.id: assistant for assistant in rows}
    return [
        agents_by_id[assistant_id]
        for assistant_id in assistant_ids
        if assistant_id in agents_by_id
    ]


async def _group_agent_participants(
    db: AsyncSession,
    group_id: UUID,
    current_agent_id: UUID,
) -> list[GroupAgentParticipant]:
    rows = (
        await db.execute(
            select(GroupAgent, Agent)
            .join(Agent, Agent.id == GroupAgent.agent_id)
            .where(
                GroupAgent.group_id == group_id,
                GroupAgent.status == "active",
                Agent.status == "active",
            )
            .order_by(GroupAgent.joined_at.asc(), GroupAgent.id.asc())
        )
    ).all()
    return [
        GroupAgentParticipant(
            agent_id=agent.id,
            display_name=group_agent.display_name or agent.name,
            agent_name=agent.name,
            role=group_agent.role,
            topology_role=group_agent.topology_role,
            response_mode=group_agent.response_mode,
            is_self=agent.id == current_agent_id,
        )
        for group_agent, agent in rows
    ]


async def _group_human_participants(
    db: AsyncSession,
    group_id: UUID,
) -> list[GroupHumanParticipant]:
    rows = (
        await db.execute(
            select(GroupMember, User)
            .join(User, User.id == GroupMember.user_id)
            .where(GroupMember.group_id == group_id, GroupMember.status == "active")
            .order_by(GroupMember.joined_at.asc(), GroupMember.id.asc())
        )
    ).all()
    return [
        GroupHumanParticipant(
            user_id=user.id,
            display_name=user.name,
            role=group_member.role,
        )
        for group_member, user in rows
    ]


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
    assistant_agents: list[Agent],
    group_agent_participants: list[GroupAgentParticipant],
    group_human_participants: list[GroupHumanParticipant],
    runtime_limits: dict[str, int],
) -> str:
    parts: list[str] = [agent.system_prompt]

    if group is not None:
        parts.append(_render_group_context(group))
        parts.append(
            _render_group_participants(
                group_agent_participants,
                group_human_participants,
            )
        )
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
    parts.append(_render_skills(skills))
    if assistant_agents:
        parts.append(_render_assistant_agents(assistant_agents))

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
    lines.append(f"- communication_mode: {group.communication_mode}")
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


def _render_group_participants(
    agent_participants: list[GroupAgentParticipant],
    human_participants: list[GroupHumanParticipant],
) -> str:
    lines = ["Group participants:"]
    if agent_participants:
        lines.append("Active agents:")
        for participant in agent_participants:
            markers = ["you"] if participant.is_self else []
            details = [f"agent_name={participant.agent_name}"]
            if participant.role:
                details.append(f"role={participant.role}")
            if participant.topology_role:
                details.append(f"topology_role={participant.topology_role}")
            details.append(f"response_mode={participant.response_mode}")
            suffix = f" ({', '.join(markers + details)})"
            lines.append(f"- @{participant.display_name}{suffix}")
    else:
        lines.append("Active agents: none")

    if human_participants:
        lines.append("Active human members:")
        for human in human_participants:
            lines.append(f"- {human.display_name} ({human.role})")
    else:
        lines.append("Active human members: none")

    lines.append(
        "Use the listed @display names when addressing another agent. Do not claim "
        "that you are the only agent if other active agents are listed."
    )
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
        "workspace with output limits, destructive command guards, and a generous default "
        "timeout that can be overridden with timeout_seconds. Fetch is "
        "limited to bounded text http/https GET requests. WebSearch uses configured Tavily "
        "credentials or a configured Playwright-backed search service when available and "
        "otherwise returns a controlled setup-required tool result. AskUser returns a "
        "non-blocking WAITING_FOR_USER result. AgentAsTool dispatches tasks only to explicitly "
        "bound assistant agents as visible group @mentions when group context is available; "
        "direct/private invocations return a controlled group-context-required result. "
        "Media generation, plan-exit, "
        "and transient todo tools are executable bounded tool calls; if no provider or persistence "
        "contract is configured they return "
        "truthful controlled tool results instead of pretending work completed. SkillManager "
        "can inspect mounted skill metadata and records activation intent only; it does not "
        "load arbitrary code."
    )


def _render_assistant_agents(assistant_agents: list[Agent]) -> str:
    lines = [
        "Bound assistant agents:",
        "When the user asks you to call, ask, delegate to, hand off to, or use one of "
        "these assistants, you must call the AgentAsTool provider-native tool before "
        "doing the assistant's work yourself. Pass the user's requested deliverable as "
        "the task, including any URLs or artifacts to produce. In group chat this creates "
        "a visible @mention dispatch to the selected assistant, who must already be a "
        "member of the same group and will respond through normal group routing; "
        "direct/private invocation cannot call assistants hidden in the backend.",
    ]
    for assistant in assistant_agents:
        description = f" — {assistant.description}" if assistant.description else ""
        lines.append(f"- @{assistant.name} ({assistant.id}){description}")
    return "\n".join(lines)


def _render_runtime_limits(runtime_limits: dict[str, int]) -> str:
    lines = ["Runtime limits:"]
    for key in sorted(runtime_limits):
        lines.append(f"- {key}: {runtime_limits[key]}")
    return "\n".join(lines)


def _render_skills(skills: list[Skill]) -> str:
    if not skills:
        return (
            "Mounted skills:\n"
            "- none\n"
            "Mounted skills are selected on this agent and are independent of "
            "workspace files."
        )
    rendered = [
        "Mounted skills:",
        "Full skill instructions are loaded only through SkillManager inspect/activate "
        "runtime tool calls; initial context lists metadata only.",
        "Mounted skills are selected on this agent and are independent of "
        "workspace files.",
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
