from typing import Literal

from pydantic import BaseModel, Field

from app.core.exceptions import AgentChatError

ToolPolicy = Literal[
    "read",
    "write",
    "execute",
    "network",
    "media",
    "planning",
    "orchestration",
]
ToolRuntimeStatus = Literal["available", "planned", "sandbox_required", "disabled"]
EXECUTABLE_TOOL_IDS: frozenset[str] = frozenset(
    {"read", "write", "edit", "glob", "grep", "bash", "fetch"}
)


class BuiltinToolRead(BaseModel):
    id: str
    name: str
    description: str
    policy: ToolPolicy
    requires_workspace: bool = False
    requires_sandbox: bool = False
    runtime_status: ToolRuntimeStatus = "planned"


class AgentToolSelection(BaseModel):
    enabled: bool = True
    policy: ToolPolicy | None = None


class AgentToolConfig(BaseModel):
    tools: dict[str, AgentToolSelection] = Field(default_factory=dict)


BUILTIN_TOOLS: tuple[BuiltinToolRead, ...] = (
    BuiltinToolRead(
        id="read",
        name="Read",
        description="Read files from the bound workspace.",
        policy="read",
        requires_workspace=True,
        runtime_status="available",
    ),
    BuiltinToolRead(
        id="write",
        name="Write",
        description="Create or replace files in the bound workspace.",
        policy="write",
        requires_workspace=True,
        runtime_status="available",
    ),
    BuiltinToolRead(
        id="edit",
        name="Edit",
        description="Patch existing files in the bound workspace.",
        policy="write",
        requires_workspace=True,
        runtime_status="available",
    ),
    BuiltinToolRead(
        id="glob",
        name="Glob",
        description="Find files in the bound workspace by pattern.",
        policy="read",
        requires_workspace=True,
        runtime_status="available",
    ),
    BuiltinToolRead(
        id="grep",
        name="Grep",
        description="Search file contents in the bound workspace.",
        policy="read",
        requires_workspace=True,
        runtime_status="available",
    ),
    BuiltinToolRead(
        id="bash",
        name="Bash",
        description="Run guarded shell commands in the bound workspace.",
        policy="execute",
        requires_workspace=True,
        runtime_status="available",
    ),
    BuiltinToolRead(
        id="ask_user",
        name="AskUser",
        description="Ask the user for clarification or approval.",
        policy="planning",
        runtime_status="planned",
    ),
    BuiltinToolRead(
        id="web_search",
        name="WebSearch",
        description="Search the web for current information.",
        policy="network",
        runtime_status="planned",
    ),
    BuiltinToolRead(
        id="fetch",
        name="Fetch",
        description="Fetch and inspect a specific URL.",
        policy="network",
        runtime_status="available",
    ),
    BuiltinToolRead(
        id="run_sub_agent",
        name="RunSubAgent",
        description="Delegate read-only exploration to a sub-agent.",
        policy="orchestration",
        runtime_status="planned",
    ),
    BuiltinToolRead(
        id="generate_image",
        name="GenerateImage",
        description="Generate images through a media provider.",
        policy="media",
        runtime_status="planned",
    ),
    BuiltinToolRead(
        id="generate_video",
        name="GenerateVideo",
        description="Generate videos through a media provider.",
        policy="media",
        runtime_status="planned",
    ),
    BuiltinToolRead(
        id="skill_manager",
        name="SkillManager",
        description="Inspect and activate mounted skills.",
        policy="orchestration",
        runtime_status="planned",
    ),
    BuiltinToolRead(
        id="todo_write",
        name="TodoWrite",
        description="Track multi-step agent tasks.",
        policy="planning",
        runtime_status="planned",
    ),
    BuiltinToolRead(
        id="exit_plan_mode",
        name="ExitPlanMode",
        description="Request user approval after planning.",
        policy="planning",
        runtime_status="planned",
    ),
)

_TOOL_BY_ID: dict[str, BuiltinToolRead] = {tool.id: tool for tool in BUILTIN_TOOLS}
DEFAULT_ENABLED_TOOL_IDS: frozenset[str] = frozenset({"read", "glob", "grep"})


def list_builtin_tools() -> list[BuiltinToolRead]:
    return list(BUILTIN_TOOLS)


def normalize_tool_config(config: AgentToolConfig | None) -> dict[str, object]:
    if config is None:
        selections = {
            tool.id: AgentToolSelection(
                enabled=tool.id in DEFAULT_ENABLED_TOOL_IDS,
                policy=tool.policy,
            )
            for tool in BUILTIN_TOOLS
        }
    else:
        selections = {}
        for tool_id, selection in config.tools.items():
            tool = _TOOL_BY_ID.get(tool_id)
            if tool is None:
                raise AgentChatError(f"unknown tool: {tool_id}")
            selections[tool_id] = AgentToolSelection(
                enabled=selection.enabled,
                policy=selection.policy or tool.policy,
            )
        for tool in BUILTIN_TOOLS:
            selections.setdefault(
                tool.id,
                AgentToolSelection(
                    enabled=tool.id in DEFAULT_ENABLED_TOOL_IDS,
                    policy=tool.policy,
                ),
            )
    return AgentToolConfig(tools=selections).model_dump(mode="json")


def selected_tool_names(tool_config: dict[str, object] | None) -> list[str]:
    config = AgentToolConfig.model_validate(tool_config or normalize_tool_config(None))
    return [
        _TOOL_BY_ID[tool_id].name
        for tool_id, selection in config.tools.items()
        if selection.enabled and tool_id in _TOOL_BY_ID
    ]


def executable_tool_names(tool_config: dict[str, object] | None) -> list[str]:
    config = AgentToolConfig.model_validate(tool_config or normalize_tool_config(None))
    return [
        _TOOL_BY_ID[tool_id].name
        for tool_id, selection in config.tools.items()
        if selection.enabled
        and tool_id in _TOOL_BY_ID
        and tool_id in EXECUTABLE_TOOL_IDS
        and _TOOL_BY_ID[tool_id].runtime_status == "available"
    ]


def saved_only_tool_names(tool_config: dict[str, object] | None) -> list[str]:
    config = AgentToolConfig.model_validate(tool_config or normalize_tool_config(None))
    return [
        _TOOL_BY_ID[tool_id].name
        for tool_id, selection in config.tools.items()
        if selection.enabled
        and tool_id in _TOOL_BY_ID
        and tool_id not in EXECUTABLE_TOOL_IDS
    ]


def enabled_tool_names(tool_config: dict[str, object] | None) -> list[str]:
    """Backward-compatible name for persisted/selected tool names."""

    return selected_tool_names(tool_config)
