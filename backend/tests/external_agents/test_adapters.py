from __future__ import annotations

import sys
from pathlib import Path
from typing import cast
from uuid import uuid4

import pytest
from sqlalchemy import Table, select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.exceptions import AgentChatError
from app.external_agents.adapters import (
    AdapterCommand,
    ExternalRuntimeConfig,
    build_command,
    normalize_external_runtime,
    parse_stdout_line,
)
from app.external_agents.runtime import run_external_agent_stream
from app.models.external_agent_run import ExternalAgentRun


def test_codex_command_uses_exec_full_auto(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr("app.external_agents.adapters.shutil.which", lambda value: value)
    command = build_command(ExternalRuntimeConfig(adapter="codex"), "do work")
    assert command.argv == ["codex", "exec", "--sandbox", "danger-full-access", "do work"]
    assert command.redacted_argv[-1] == "<prompt>"


def test_claude_command_uses_stream_json_and_bypass(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr("app.external_agents.adapters.shutil.which", lambda value: value)
    command = build_command(
        ExternalRuntimeConfig(adapter="claude_code", executable="claude", max_turns=7),
        "do work",
    )
    assert command.argv == [
        "claude",
        "-p",
        "--output-format",
        "stream-json",
        "--permission-mode",
        "bypassPermissions",
        "--max-turns",
        "7",
        "do work",
    ]
    assert command.redacted_argv[-1] == "<prompt>"


def test_claude_stream_json_parser_extracts_text() -> None:
    assert parse_stdout_line(
        "claude_code",
        '{"type":"content_block_delta","delta":{"type":"text_delta","text":"hi"}}',
    ) == ["hi"]
    assert parse_stdout_line(
        "claude_code",
        '{"type":"assistant","message":{"content":[{"type":"text","text":"done"}]}}',
    ) == ["done"]


def test_external_runtime_validation_requires_known_adapter() -> None:
    with pytest.raises(AgentChatError):
        normalize_external_runtime({"adapter": "pi"})


@pytest.mark.asyncio
async def test_external_run_stream_persists_audit_row(
    db_session: AsyncSession, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    script = tmp_path / "fake_agent.py"
    script.write_text("import sys\nprint('reply:' + sys.argv[-1])\n", encoding="utf-8")

    def fake_build_command(config: ExternalRuntimeConfig, prompt: str) -> AdapterCommand:
        _ = config
        return AdapterCommand(
            argv=[sys.executable, str(script), prompt],
            redacted_argv=[sys.executable, str(script), "<prompt>"],
        )

    monkeypatch.setattr("app.external_agents.runtime.build_command", fake_build_command)
    conn = await db_session.connection()
    await conn.run_sync(cast(Table, ExternalAgentRun.__table__).create, checkfirst=True)
    owner_id = uuid4()
    agent_id = uuid4()
    chunks: list[str] = []
    run_events: list[dict[str, object]] = []

    async for event in run_external_agent_stream(
        db_session,
        owner_id=owner_id,
        group_id=uuid4(),
        agent_id=agent_id,
        thread_id=uuid4(),
        config=ExternalRuntimeConfig(adapter="codex"),
        cwd=tmp_path,
        prompt="task",
    ):
        if event.kind == "token" and isinstance(event.data, str):
            chunks.append(event.data)
        if event.kind == "run" and isinstance(event.data, dict):
            run_events.append(event.data)

    assert "".join(chunks) == "reply:task\n"
    assert run_events[-1]["status"] == "completed"
    row = await db_session.scalar(
        select(ExternalAgentRun).where(ExternalAgentRun.agent_id == agent_id)
    )
    assert row is not None
    assert row.status == "completed"
    assert row.argv[-1] == "<prompt>"
    assert row.stdout_tail == "reply:task\n"


@pytest.mark.asyncio
async def test_external_run_uses_isolated_cli_home(
    db_session: AsyncSession,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    script = tmp_path / "fake_agent_env.py"
    script.write_text(
        "\n".join(
            [
                "import os",
                "print('codex_home=' + os.environ.get('CODEX_HOME', ''))",
                "print('claude_home=' + os.environ.get('CLAUDE_HOME', ''))",
                "print('home=' + os.environ.get('HOME', ''))",
                "print('marker=' + os.environ.get('AG_SWARMER_EXTERNAL_AGENT', ''))",
            ]
        )
        + "\n",
        encoding="utf-8",
    )

    def fake_build_command(config: ExternalRuntimeConfig, prompt: str) -> AdapterCommand:
        _ = (config, prompt)
        return AdapterCommand(
            argv=[sys.executable, str(script)],
            redacted_argv=[sys.executable, str(script)],
        )

    monkeypatch.setenv("CODEX_HOME", str(tmp_path / "host-codex"))
    monkeypatch.setenv("CLAUDE_HOME", str(tmp_path / "host-claude"))
    monkeypatch.setattr("app.external_agents.runtime.build_command", fake_build_command)
    conn = await db_session.connection()
    await conn.run_sync(cast(Table, ExternalAgentRun.__table__).create, checkfirst=True)
    chunks: list[str] = []

    async for event in run_external_agent_stream(
        db_session,
        owner_id=uuid4(),
        group_id=uuid4(),
        agent_id=uuid4(),
        thread_id=uuid4(),
        config=ExternalRuntimeConfig(adapter="codex"),
        cwd=tmp_path,
        prompt="task",
    ):
        if event.kind == "token" and isinstance(event.data, str):
            chunks.append(event.data)

    output = "".join(chunks)
    assert "host-codex" not in output
    assert "host-claude" not in output
    assert "codex_home=" in output
    assert "claude_home=" in output
    assert "marker=1" in output
