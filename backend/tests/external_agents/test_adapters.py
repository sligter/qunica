from __future__ import annotations

import json
import sys
from collections.abc import AsyncIterator
from pathlib import Path
from uuid import uuid4

import pytest
import pytest_asyncio
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker, create_async_engine

from app.core.exceptions import AgentChatError
from app.external_agents.adapters import AcpRuntimeConfig, normalize_acp_runtime
from app.external_agents.discovery import discover_acp_runtime_presets
from app.external_agents.runtime import (
    _mode_config_option_ids,
    _thinking_config_option_ids,
    _windows_hidden_subprocess_kwargs,
    run_acp_agent_stream,
)
from app.models.external_agent_run import ExternalAgentRun


@pytest_asyncio.fixture
async def acp_db_session() -> AsyncIterator[AsyncSession]:
    engine = create_async_engine("sqlite+aiosqlite:///:memory:")
    async with engine.begin() as conn:
        await conn.run_sync(ExternalAgentRun.__table__.create)
    session_factory = async_sessionmaker(engine, class_=AsyncSession, expire_on_commit=False)
    async with session_factory() as session:
        yield session
    await engine.dispose()


def test_acp_runtime_validation_accepts_command_and_args() -> None:
    config = normalize_acp_runtime(
        {
            "command": sys.executable,
            "args": ["agent.py", "--acp"],
            "env": {"API_KEY": "secret"},
            "timeout_seconds": 10,
            "permission_policy": "auto_allow",
            "profile": "codex",
            "model": "gpt-test",
            "mode": "auto-edit",
            "thinking_effort": "high",
            "config_options": {"custom": True},
        }
    )

    assert config.command == sys.executable
    assert config.profile == "codex"
    assert config.args == ["agent.py", "--acp"]
    assert config.env == {"API_KEY": "secret"}
    assert config.timeout_seconds == 10
    assert config.permission_policy == "auto_allow"
    assert config.model == "gpt-test"
    assert config.mode == "auto-edit"
    assert config.thinking_effort == "high"
    assert config.config_options == {"custom": True}


def test_acp_runtime_validation_rejects_legacy_adapter() -> None:
    with pytest.raises(AgentChatError, match="external CLI adapters are deprecated"):
        normalize_acp_runtime({"adapter": "codex"})


@pytest.mark.skipif(sys.platform != "win32", reason="Windows-only console flags")
def test_acp_runtime_hides_windows_console() -> None:
    import subprocess

    kwargs = _windows_hidden_subprocess_kwargs()

    assert kwargs["creationflags"] & subprocess.CREATE_NO_WINDOW
    startupinfo = kwargs["startupinfo"]
    assert startupinfo.dwFlags & subprocess.STARTF_USESHOWWINDOW
    assert startupinfo.wShowWindow == subprocess.SW_HIDE


def test_acp_runtime_env_rejects_isolation_overrides() -> None:
    with pytest.raises(AgentChatError, match="CODEX_HOME"):
        normalize_acp_runtime({"command": sys.executable, "env": {"CODEX_HOME": "host"}})


def test_acp_runtime_discovery_detects_path_command(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _isolate_discovery_env(tmp_path, monkeypatch)
    executable_name = "codex-acp.cmd" if sys.platform == "win32" else "codex-acp"
    executable = tmp_path / executable_name
    executable.write_text("", encoding="utf-8")
    if sys.platform != "win32":
        executable.chmod(0o755)
    monkeypatch.setenv("PATH", str(tmp_path))

    presets = discover_acp_runtime_presets()
    codex = next(preset for preset in presets if preset.id == "codex")

    assert codex.installed is True
    assert codex.command is not None
    assert codex.command.lower().endswith(executable_name)


def test_acp_runtime_discovery_falls_back_to_npx(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _isolate_discovery_env(tmp_path, monkeypatch)
    npx_name = "npx.cmd" if sys.platform == "win32" else "npx"
    npx = tmp_path / npx_name
    npx.write_text("", encoding="utf-8")
    if sys.platform != "win32":
        npx.chmod(0o755)
    monkeypatch.setenv("PATH", str(tmp_path))

    presets = discover_acp_runtime_presets()
    codex = next(preset for preset in presets if preset.id == "codex")
    claude = next(preset for preset in presets if preset.id == "claude")

    assert codex.installed is False
    assert codex.command is not None
    assert codex.command.lower().endswith(npx_name)
    assert codex.args == ["@zed-industries/codex-acp"]
    assert claude.installed is False
    assert claude.command is not None
    assert claude.command.lower().endswith(npx_name)
    assert claude.args == ["@agentclientprotocol/claude-agent-acp"]


def test_acp_runtime_discovery_scans_user_bin_roots(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _isolate_discovery_env(tmp_path, monkeypatch)
    executable_name = "claude-agent-acp.cmd" if sys.platform == "win32" else "claude-agent-acp"
    executable = tmp_path / "roaming" / "npm" / executable_name
    executable.parent.mkdir(parents=True)
    executable.write_text("", encoding="utf-8")
    if sys.platform != "win32":
        executable.chmod(0o755)

    presets = discover_acp_runtime_presets()
    claude = next(preset for preset in presets if preset.id == "claude")

    assert claude.installed is True
    assert claude.command is not None
    assert claude.command.lower().endswith(executable_name)


def test_acp_runtime_presets_use_adapter_option_values(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _isolate_discovery_env(tmp_path, monkeypatch)

    presets = discover_acp_runtime_presets()
    codex = next(preset for preset in presets if preset.id == "codex")
    claude = next(preset for preset in presets if preset.id == "claude")

    assert [option.value for option in codex.mode_options] == [
        "read-only",
        "auto",
        "full-access",
    ]
    assert codex.model_options == []
    assert _thinking_config_option_ids("codex")[0] == "reasoning_effort"
    assert _mode_config_option_ids("codex")[0] == "mode"
    assert claude.model_options == []
    assert "acceptEdits" in {option.value for option in claude.mode_options}
    assert "max" in {option.value for option in claude.thinking_effort_options}
    assert _thinking_config_option_ids("claude")[0] == "effort"
    assert _mode_config_option_ids("claude")[0] == "mode"


def test_acp_runtime_presets_read_codex_models_from_config_files(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _isolate_discovery_env(tmp_path, monkeypatch)
    codex_home = tmp_path / "home" / ".codex"
    codex_home.mkdir(parents=True)
    (codex_home / "config.toml").write_text(
        '\n'.join(
            [
                'model = "gpt-current"',
                "",
                "[profiles.fast]",
                'model = "gpt-fast"',
            ]
        ),
        encoding="utf-8",
    )
    (codex_home / "models_cache.json").write_text(
        json.dumps(
            {
                "models": [
                    {
                        "slug": "gpt-current",
                        "display_name": "GPT Current",
                        "description": "Current configured model.",
                        "visibility": "list",
                    },
                    {
                        "slug": "gpt-cache",
                        "display_name": "GPT Cache",
                        "visibility": "list",
                    },
                    {
                        "slug": "codex-auto-review",
                        "display_name": "Codex Auto Review",
                        "visibility": "hide",
                    },
                ]
            }
        ),
        encoding="utf-8",
    )

    presets = discover_acp_runtime_presets()
    codex = next(preset for preset in presets if preset.id == "codex")
    option_by_value = {option.value: option for option in codex.model_options}

    assert codex.default_model == "gpt-current"
    assert [option.value for option in codex.model_options] == [
        "gpt-current",
        "gpt-cache",
        "gpt-fast",
    ]
    assert option_by_value["gpt-current"].label == "GPT Current"
    assert option_by_value["gpt-fast"].label == "GPT Fast"
    assert "codex-auto-review" not in option_by_value


def test_acp_runtime_presets_read_claude_models_from_settings(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _isolate_discovery_env(tmp_path, monkeypatch)
    claude_home = tmp_path / "home" / ".claude"
    claude_home.mkdir(parents=True)
    (claude_home / "settings.json").write_text(
        json.dumps(
            {
                "model": "opus[1m]",
                "env": {
                    "ANTHROPIC_MODEL": "claude-opus-4-8",
                    "CLAUDE_MODEL_CONFIG": json.dumps(
                        {
                            "availableModels": [
                                {
                                    "modelId": "claude-sonnet-4-8",
                                    "name": "Claude Sonnet 4.8",
                                }
                            ],
                            "modelOverrides": {"haiku": "claude-haiku-4-5"},
                        }
                    ),
                },
            }
        ),
        encoding="utf-8",
    )

    presets = discover_acp_runtime_presets()
    claude = next(preset for preset in presets if preset.id == "claude")
    option_by_value = {option.value: option for option in claude.model_options}

    assert claude.default_model == "opus[1m]"
    assert [option.value for option in claude.model_options] == [
        "opus[1m]",
        "claude-opus-4-8",
        "claude-sonnet-4-8",
        "claude-haiku-4-5",
    ]
    assert option_by_value["opus[1m]"].label == "Opus (1M context)"
    assert option_by_value["claude-sonnet-4-8"].label == "Claude Sonnet 4.8"


def _isolate_discovery_env(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("PATH", str(tmp_path / "empty-path"))
    monkeypatch.setenv("APPDATA", str(tmp_path / "roaming"))
    monkeypatch.setenv("LOCALAPPDATA", str(tmp_path / "local"))
    monkeypatch.setenv("USERPROFILE", str(tmp_path / "home"))
    monkeypatch.setenv("ProgramFiles", str(tmp_path / "program-files"))
    monkeypatch.setenv("ProgramFiles(x86)", str(tmp_path / "program-files-x86"))
    monkeypatch.delenv("CODEX_HOME", raising=False)
    monkeypatch.delenv("CLAUDE_CONFIG_DIR", raising=False)
    monkeypatch.delenv("CLAUDE_HOME", raising=False)
    monkeypatch.delenv("npm_config_cache", raising=False)
    monkeypatch.delenv("NPM_CONFIG_CACHE", raising=False)


@pytest.mark.asyncio
async def test_acp_run_stream_persists_audit_row(
    acp_db_session: AsyncSession,
    tmp_path: Path,
) -> None:
    script = _write_fake_acp_agent(tmp_path)
    owner_id = uuid4()
    agent_id = uuid4()
    chunks: list[str] = []
    thoughts: list[str] = []
    run_events: list[dict[str, object]] = []

    async for event in run_acp_agent_stream(
        acp_db_session,
        owner_id=owner_id,
        group_id=uuid4(),
        agent_id=agent_id,
        thread_id=uuid4(),
        config=AcpRuntimeConfig(
            command=sys.executable,
            args=[str(script)],
            env={},
            timeout_seconds=10,
        ),
        cwd=tmp_path,
        prompt="task",
    ):
        if event.kind == "token" and isinstance(event.data, str):
            chunks.append(event.data)
        if event.kind == "reasoning" and isinstance(event.data, str):
            thoughts.append(event.data)
        if event.kind == "run" and isinstance(event.data, dict):
            run_events.append(event.data)

    assert "".join(chunks) == "reply:task"
    assert thoughts == ["thinking"]
    assert run_events[-1]["status"] == "completed"
    row = await acp_db_session.scalar(
        select(ExternalAgentRun).where(ExternalAgentRun.agent_id == agent_id)
    )
    assert row is not None
    assert row.status == "completed"
    assert row.adapter == "acp"
    assert row.argv == [sys.executable, str(script)]
    assert row.stdout_tail is None


@pytest.mark.asyncio
async def test_acp_run_stream_accepts_large_single_line_message(
    acp_db_session: AsyncSession,
    tmp_path: Path,
) -> None:
    script = _write_large_message_acp_agent(tmp_path)
    chunks: list[str] = []

    async for event in run_acp_agent_stream(
        acp_db_session,
        owner_id=uuid4(),
        group_id=uuid4(),
        agent_id=uuid4(),
        thread_id=uuid4(),
        config=AcpRuntimeConfig(
            command=sys.executable,
            args=[str(script)],
            env={},
            timeout_seconds=10,
        ),
        cwd=tmp_path,
        prompt="task",
    ):
        if event.kind == "token" and isinstance(event.data, str):
            chunks.append(event.data)

    assert "".join(chunks) == "x" * (128 * 1024)


@pytest.mark.asyncio
async def test_acp_run_stream_emits_usage_update(
    acp_db_session: AsyncSession,
    tmp_path: Path,
) -> None:
    script = _write_usage_acp_agent(tmp_path)
    usages: list[dict[str, object]] = []

    async for event in run_acp_agent_stream(
        acp_db_session,
        owner_id=uuid4(),
        group_id=uuid4(),
        agent_id=uuid4(),
        thread_id=uuid4(),
        config=AcpRuntimeConfig(
            command=sys.executable,
            args=[str(script)],
            env={},
            timeout_seconds=10,
        ),
        cwd=tmp_path,
        prompt="task",
    ):
        if event.kind == "usage" and isinstance(event.data, dict):
            usages.append(event.data)

    assert usages == [{"used": 42_000, "size": 200_000}]


@pytest.mark.asyncio
async def test_acp_run_applies_session_settings(
    acp_db_session: AsyncSession,
    tmp_path: Path,
) -> None:
    script = _write_settings_acp_agent(tmp_path)
    chunks: list[str] = []

    async for event in run_acp_agent_stream(
        acp_db_session,
        owner_id=uuid4(),
        group_id=uuid4(),
        agent_id=uuid4(),
        thread_id=uuid4(),
        config=AcpRuntimeConfig(
            command=sys.executable,
            args=[str(script)],
            env={},
            timeout_seconds=10,
            profile="codex",
            model="gpt-test",
            mode="auto-edit",
            thinking_effort="xhigh",
            config_options={"sandbox": True},
        ),
        cwd=tmp_path,
        prompt="task",
    ):
        if event.kind == "token" and isinstance(event.data, str):
            chunks.append(event.data)

    output = "\n".join(chunks)
    assert "model=gpt-test" in output
    assert "mode=auto-edit" in output
    assert "config:reasoning_effort=xhigh" in output
    assert "config:sandbox=True" in output


@pytest.mark.asyncio
async def test_acp_run_uses_isolated_home(
    acp_db_session: AsyncSession,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    script = _write_env_acp_agent(tmp_path)
    monkeypatch.setenv("CODEX_HOME", str(tmp_path / "host-codex"))
    monkeypatch.setenv("CLAUDE_HOME", str(tmp_path / "host-claude"))
    monkeypatch.setenv("CLAUDE_CONFIG_DIR", str(tmp_path / "host-claude-config"))
    monkeypatch.setenv("USERPROFILE", str(tmp_path / "host-user"))
    chunks: list[str] = []

    async for event in run_acp_agent_stream(
        acp_db_session,
        owner_id=uuid4(),
        group_id=uuid4(),
        agent_id=uuid4(),
        thread_id=uuid4(),
        config=AcpRuntimeConfig(
            command=sys.executable,
            args=[str(script)],
            env={},
            timeout_seconds=10,
        ),
        cwd=tmp_path,
        prompt="task",
    ):
        if event.kind == "token" and isinstance(event.data, str):
            chunks.append(event.data)

    output = "".join(chunks)
    assert "host-codex" not in output
    assert "host-claude" not in output
    assert "host-user" not in output
    assert "AG_SWARMER_ACP_AGENT=1" in output
    assert "CODEX_HOME=" in output
    assert "CLAUDE_HOME=" in output
    assert "CLAUDE_CONFIG_DIR=" in output
    assert "USERPROFILE=" in output


@pytest.mark.asyncio
async def test_acp_run_claude_profile_inherits_host_auth_env(
    acp_db_session: AsyncSession,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    script = _write_env_acp_agent(tmp_path)
    monkeypatch.setenv("CLAUDE_HOME", str(tmp_path / "host-claude"))
    monkeypatch.setenv("CLAUDE_CONFIG_DIR", str(tmp_path / "host-claude-config"))
    monkeypatch.setenv("USERPROFILE", str(tmp_path / "host-user"))
    chunks: list[str] = []

    async for event in run_acp_agent_stream(
        acp_db_session,
        owner_id=uuid4(),
        group_id=uuid4(),
        agent_id=uuid4(),
        thread_id=uuid4(),
        config=AcpRuntimeConfig(
            command=sys.executable,
            args=[str(script)],
            env={},
            timeout_seconds=10,
            profile="claude",
        ),
        cwd=tmp_path,
        prompt="task",
    ):
        if event.kind == "token" and isinstance(event.data, str):
            chunks.append(event.data)

    output = "".join(chunks)
    assert f"CLAUDE_HOME={tmp_path / 'host-claude'}" in output
    assert f"CLAUDE_CONFIG_DIR={tmp_path / 'host-claude-config'}" in output
    assert f"USERPROFILE={tmp_path / 'host-user'}" in output
    assert "AG_SWARMER_ACP_AGENT=1" in output


@pytest.mark.asyncio
async def test_acp_run_timeout_persists_timeout(
    acp_db_session: AsyncSession,
    tmp_path: Path,
) -> None:
    script = _write_sleep_acp_agent(tmp_path)
    agent_id = uuid4()

    with pytest.raises(AgentChatError, match="timed out"):
        async for _event in run_acp_agent_stream(
            acp_db_session,
            owner_id=uuid4(),
            group_id=uuid4(),
            agent_id=agent_id,
            thread_id=uuid4(),
            config=AcpRuntimeConfig(
                command=sys.executable,
                args=[str(script)],
                env={},
                timeout_seconds=1,
            ),
            cwd=tmp_path,
            prompt="task",
        ):
            pass

    row = await acp_db_session.scalar(
        select(ExternalAgentRun).where(ExternalAgentRun.agent_id == agent_id)
    )
    assert row is not None
    assert row.status == "timeout"


def _write_fake_acp_agent(tmp_path: Path) -> Path:
    script = tmp_path / "fake_acp_agent.py"
    script.write_text(
        _FAKE_AGENT_SOURCE.format(
            extra_methods="",
            prompt_body=(
                "text = ''.join(getattr(block, 'text', '') for block in prompt)\n"
                "        await self._conn.session_update(\n"
                "            session_id=session_id,\n"
                "            update=update_agent_thought(text_block('thinking')),\n"
                "        )\n"
                "        await self._conn.session_update(\n"
                "            session_id=session_id,\n"
                "            update=update_agent_message(text_block('reply:' + text)),\n"
                "        )"
            )
        ),
        encoding="utf-8",
    )
    return script


def _write_usage_acp_agent(tmp_path: Path) -> Path:
    script = tmp_path / "usage_acp_agent.py"
    script.write_text(
        _FAKE_AGENT_SOURCE.format(
            extra_methods="",
            prompt_body=(
                "await self._conn.session_update(\n"
                "            session_id=session_id,\n"
                "            update=UsageUpdate(used=42_000, size=200_000, "
                "session_update='usage_update'),\n"
                "        )\n"
                "        await self._conn.session_update(\n"
                "            session_id=session_id,\n"
                "            update=update_agent_message(text_block('done')),\n"
                "        )"
            ),
        ),
        encoding="utf-8",
    )
    return script


def _write_large_message_acp_agent(tmp_path: Path) -> Path:
    script = tmp_path / "large_message_acp_agent.py"
    script.write_text(
        _FAKE_AGENT_SOURCE.format(
            extra_methods="",
            prompt_body=(
                "text = 'x' * (128 * 1024)\n"
                "        await self._conn.session_update(\n"
                "            session_id=session_id,\n"
                "            update=update_agent_message(text_block(text)),\n"
                "        )"
            ),
        ),
        encoding="utf-8",
    )
    return script


def _write_env_acp_agent(tmp_path: Path) -> Path:
    script = tmp_path / "env_acp_agent.py"
    marker_line = (
        "            'AG_SWARMER_ACP_AGENT=' "
        "+ os.environ.get('AG_SWARMER_ACP_AGENT', ''),\n"
    )
    script.write_text(
        _FAKE_AGENT_SOURCE.format(
            extra_methods="",
            prompt_body=(
                "import os\n"
                "        text = '\\n'.join([\n"
                f"{marker_line}"
                "            'HOME=' + os.environ.get('HOME', ''),\n"
                "            'USERPROFILE=' + os.environ.get('USERPROFILE', ''),\n"
                "            'CODEX_HOME=' + os.environ.get('CODEX_HOME', ''),\n"
                "            'CLAUDE_HOME=' + os.environ.get('CLAUDE_HOME', ''),\n"
                "            'CLAUDE_CONFIG_DIR=' "
                "+ os.environ.get('CLAUDE_CONFIG_DIR', ''),\n"
                "        ])\n"
                "        await self._conn.session_update(\n"
                "            session_id=session_id,\n"
                "            update=update_agent_message(text_block(text)),\n"
                "        )"
            )
        ),
        encoding="utf-8",
    )
    return script


def _write_settings_acp_agent(tmp_path: Path) -> Path:
    script = tmp_path / "settings_acp_agent.py"
    script.write_text(
        _FAKE_AGENT_SOURCE.format(
            extra_methods=(
                "    async def set_session_model(\n"
                "        self,\n"
                "        model_id: str,\n"
                "        session_id: str,\n"
                "        **kwargs: Any,\n"
                "    ) -> SetSessionModelResponse:\n"
                "        _ = (session_id, kwargs)\n"
                "        self.settings['model'] = model_id\n"
                "        return SetSessionModelResponse()\n\n"
                "    async def set_session_mode(\n"
                "        self,\n"
                "        mode_id: str,\n"
                "        session_id: str,\n"
                "        **kwargs: Any,\n"
                "    ) -> SetSessionModeResponse:\n"
                "        _ = (session_id, kwargs)\n"
                "        self.settings['mode'] = mode_id\n"
                "        return SetSessionModeResponse()\n\n"
                "    async def set_config_option(\n"
                "        self,\n"
                "        config_id: str,\n"
                "        session_id: str,\n"
                "        value: str | bool,\n"
                "        **kwargs: Any,\n"
                "    ) -> SetSessionConfigOptionResponse:\n"
                "        _ = (session_id, kwargs)\n"
                "        self.settings['config:' + config_id] = value\n"
                "        return SetSessionConfigOptionResponse(config_options=[])\n\n"
            ),
            prompt_body=(
                "lines = [f'{key}={value}' for key, value in sorted(self.settings.items())]\n"
                "        await self._conn.session_update(\n"
                "            session_id=session_id,\n"
                "            update=update_agent_message(text_block('\\n'.join(lines))),\n"
                "        )"
            ),
        ),
        encoding="utf-8",
    )
    return script


def _write_sleep_acp_agent(tmp_path: Path) -> Path:
    script = tmp_path / "sleep_acp_agent.py"
    script.write_text(
        _FAKE_AGENT_SOURCE.format(
            extra_methods="",
            prompt_body=(
                "import asyncio\n"
                "        await asyncio.sleep(5)\n"
                "        await self._conn.session_update(\n"
                "            session_id=session_id,\n"
                "            update=update_agent_message(text_block('late')),\n"
                "        )"
            )
        ),
        encoding="utf-8",
    )
    return script


_FAKE_AGENT_SOURCE = """\
import asyncio
from typing import Any
from uuid import uuid4

from acp import (
    Agent,
    InitializeResponse,
    NewSessionResponse,
    PromptResponse,
    run_agent,
    text_block,
    update_agent_message,
    update_agent_thought,
)
from acp.interfaces import Client
from acp.schema import ClientCapabilities, Implementation
from acp.schema import (
    SetSessionConfigOptionResponse,
    SetSessionModeResponse,
    SetSessionModelResponse,
    UsageUpdate,
)


class FakeAgent(Agent):
    _conn: Client

    def on_connect(self, conn: Client) -> None:
        self._conn = conn
        self.settings: dict[str, object] = {{}}

    async def initialize(
        self,
        protocol_version: int,
        client_capabilities: ClientCapabilities | None = None,
        client_info: Implementation | None = None,
        **kwargs: Any,
    ) -> InitializeResponse:
        _ = (client_capabilities, client_info, kwargs)
        return InitializeResponse(protocol_version=protocol_version)

    async def new_session(
        self,
        cwd: str,
        additional_directories: list[str] | None = None,
        mcp_servers: list[object] | None = None,
        **kwargs: Any,
    ) -> NewSessionResponse:
        _ = (cwd, additional_directories, mcp_servers, kwargs)
        return NewSessionResponse(session_id=uuid4().hex)

{extra_methods}
    async def prompt(
        self,
        prompt: list[object],
        session_id: str,
        message_id: str | None = None,
        **kwargs: Any,
    ) -> PromptResponse:
        _ = kwargs
        {prompt_body}
        return PromptResponse(stop_reason='end_turn', user_message_id=message_id)


async def main() -> None:
    await run_agent(FakeAgent())


if __name__ == '__main__':
    asyncio.run(main())
"""
