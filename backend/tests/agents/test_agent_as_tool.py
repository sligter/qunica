import json
from pathlib import Path
from types import SimpleNamespace
from uuid import uuid4

import pytest
from langchain_core.tools import BaseTool
from pydantic import BaseModel
from sqlalchemy.ext.asyncio import AsyncSession

from app.agents.builtin_tools import (
    AgentAssistantToolSelection,
    AgentToolConfig,
    normalize_tool_config,
)
from app.agents.context import AgentInvocationContext, build_agent_invocation_context
from app.agents.workspace_tools import (
    bind_workspace_tools,
    build_workspace_tools,
    execute_workspace_tool,
)
from app.core.exceptions import AgentChatError
from app.models.agent import Agent
from app.models.user import User
from app.schemas.agent import AgentUpdate
from app.services.agent_service import update_agent


def _local_workspace(path: Path) -> SimpleNamespace:
    return SimpleNamespace(backend_type="local", local_path=str(path.resolve()))


async def test_context_includes_bound_assistant_agents(db_session: AsyncSession) -> None:
    user = User(email=f"assistant-{uuid4().hex[:8]}@example.com", password_hash="x", name="Owner")
    db_session.add(user)
    await db_session.flush()
    helper = Agent(owner_id=user.id, name="Helper", description="Research", system_prompt="Help")
    db_session.add(helper)
    await db_session.flush()
    caller = Agent(
        owner_id=user.id,
        name="Caller",
        system_prompt="Call helpers",
        tool_config=normalize_tool_config(
            AgentToolConfig(
                assistant_agents=[AgentAssistantToolSelection(agent_id=helper.id, enabled=True)]
            )
        ),
    )
    db_session.add(caller)
    await db_session.flush()

    context = await build_agent_invocation_context(db_session, caller, user)

    assert context.assistant_agents == [helper]
    assert "AgentAsTool" in context.enabled_tools
    assert "AgentAsTool" in context.executable_tools
    assert "@Helper" in context.system_prompt
    assert (
        "direct/private invocation cannot call assistants hidden in the backend"
        in context.system_prompt
    )


async def test_agent_as_tool_uses_configured_executor() -> None:
    helper_id = uuid4()
    calls: list[tuple[str, str, str | None]] = []

    async def _executor(agent_id: str, task: str, instructions: str | None = None) -> str:
        calls.append((agent_id, task, instructions))
        return json.dumps({"status": "COMPLETED", "content": "helper answer"})

    context = object.__new__(AgentInvocationContext)
    object.__setattr__(context, "workspace", None)
    object.__setattr__(context, "executable_tools", ["AgentAsTool"])
    object.__setattr__(context, "mounted_skills", [])

    tools = build_workspace_tools(context, agent_tool_executor=_executor)
    result = await tools["AgentAsTool"].ainvoke(
        {"agent_id": str(helper_id), "task": "summarize", "instructions": "brief"}
    )

    assert calls == [(str(helper_id), "summarize", "brief")]
    assert "helper answer" in str(result)


async def test_bash_tool_accepts_timeout_above_ten_seconds_for_delegated_workflows(
    tmp_path: Path,
) -> None:
    context = object.__new__(AgentInvocationContext)
    object.__setattr__(context, "workspace", _local_workspace(tmp_path))
    object.__setattr__(context, "executable_tools", ["Bash"])
    object.__setattr__(context, "mounted_skills", [])

    tools = build_workspace_tools(context)
    result = execute_workspace_tool(
        tools,
        "Bash",
        {"command": "python -c \"print('ppt task ready')\"", "timeout_seconds": 60},
    )

    args_schema = tools["Bash"].args_schema
    assert isinstance(args_schema, type) and issubclass(args_schema, BaseModel)
    bash_schema = args_schema.model_json_schema()
    timeout_schema = bash_schema["properties"]["timeout_seconds"]

    assert "ppt task ready" in result
    assert "timeout_seconds must be between 1 and 10" not in result
    assert timeout_schema.get("maximum") is None
    assert timeout_schema.get("exclusiveMaximum") is None
    assert timeout_schema.get("default") == 600


async def test_bash_tool_allows_omitted_timeout_for_delegated_workflows(tmp_path: Path) -> None:
    context = object.__new__(AgentInvocationContext)
    object.__setattr__(context, "workspace", _local_workspace(tmp_path))
    object.__setattr__(context, "executable_tools", ["Bash"])
    object.__setattr__(context, "mounted_skills", [])

    tools = build_workspace_tools(context)
    result = execute_workspace_tool(
        tools,
        "Bash",
        {"command": "python -c \"print('default timeout remains bounded')\""},
    )

    assert "default timeout remains bounded" in result


async def test_bash_tool_rejects_excessive_timeout(tmp_path: Path) -> None:
    context = object.__new__(AgentInvocationContext)
    object.__setattr__(context, "workspace", _local_workspace(tmp_path))
    object.__setattr__(context, "executable_tools", ["Bash"])
    object.__setattr__(context, "mounted_skills", [])

    tools = build_workspace_tools(context)
    too_large_result = execute_workspace_tool(
        tools,
        "Bash",
        {"command": "python -c \"print('should not run')\"", "timeout_seconds": 3601},
    )

    assert "timeout_seconds must be between 1 and 3600" in too_large_result


async def test_agent_as_tool_and_run_sub_agent_bind_unique_provider_names() -> None:
    helper_id = uuid4()

    async def _executor(agent_id: str, task: str, instructions: str | None = None) -> str:
        _ = (agent_id, task, instructions)
        return json.dumps({"status": "COMPLETED", "content": "helper answer"})

    context = object.__new__(AgentInvocationContext)
    object.__setattr__(context, "workspace", None)
    object.__setattr__(context, "executable_tools", ["RunSubAgent", "AgentAsTool"])
    object.__setattr__(context, "mounted_skills", [])

    tools = build_workspace_tools(context, agent_tool_executor=_executor)

    assert set(tools) == {"RunSubAgent", "AgentAsTool"}
    assert [tool.name for tool in tools.values()] == ["AgentAsTool", "RunSubAgent"]
    assert len({tool.name for tool in tools.values()}) == len(tools)
    assert "helper answer" in str(
        await tools["RunSubAgent"].ainvoke(
            {"agent_id": str(helper_id), "task": "summarize", "instructions": "brief"}
        )
    )


async def test_agent_as_tool_and_run_sub_agent_bind_independently() -> None:
    agent_context = object.__new__(AgentInvocationContext)
    object.__setattr__(agent_context, "workspace", None)
    object.__setattr__(agent_context, "executable_tools", ["AgentAsTool"])
    object.__setattr__(agent_context, "mounted_skills", [])

    run_context = object.__new__(AgentInvocationContext)
    object.__setattr__(run_context, "workspace", None)
    object.__setattr__(run_context, "executable_tools", ["RunSubAgent"])
    object.__setattr__(run_context, "mounted_skills", [])

    assert set(build_workspace_tools(agent_context)) == {"AgentAsTool"}
    assert set(build_workspace_tools(run_context)) == {"RunSubAgent"}


async def test_bind_workspace_tools_passes_unique_provider_tool_names() -> None:
    class CapturingChatModel:
        bound_names: list[str] | None = None

        def bind_tools(self, bound_tools: list[BaseTool]) -> "CapturingChatModel":
            self.bound_names = [tool.name for tool in bound_tools]
            return self

    context = object.__new__(AgentInvocationContext)
    object.__setattr__(context, "workspace", None)
    object.__setattr__(context, "executable_tools", ["RunSubAgent", "AgentAsTool"])
    object.__setattr__(context, "mounted_skills", [])
    chat_model = CapturingChatModel()

    bound_model = bind_workspace_tools(chat_model, build_workspace_tools(context))

    assert bound_model is chat_model
    assert chat_model.bound_names == ["AgentAsTool", "RunSubAgent"]
    assert len(set(chat_model.bound_names)) == len(chat_model.bound_names)


async def test_agent_tool_config_rejects_duplicate_assistants() -> None:
    helper_id = uuid4()
    with pytest.raises(ValueError, match="duplicate"):
        AgentToolConfig(
            assistant_agents=[
                AgentAssistantToolSelection(agent_id=helper_id),
                AgentAssistantToolSelection(agent_id=helper_id),
            ]
        )


async def test_agent_tool_config_rejects_self_binding(
    db_session: AsyncSession,
) -> None:
    user = User(email=f"self-{uuid4().hex[:8]}@example.com", password_hash="x", name="Owner")
    db_session.add(user)
    await db_session.flush()
    agent = Agent(owner_id=user.id, name="Self", system_prompt="No recursion")
    db_session.add(agent)
    await db_session.flush()

    with pytest.raises(AgentChatError, match="cannot bind itself"):
        await update_agent(
            db_session,
            agent.id,
            AgentUpdate(
                tool_config=AgentToolConfig(
                    assistant_agents=[AgentAssistantToolSelection(agent_id=agent.id)]
                )
            ),
            user,
        )
