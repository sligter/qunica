from __future__ import annotations

import asyncio
import json
import shutil
import subprocess
from dataclasses import dataclass
from typing import Any, Literal

from app.core.exceptions import AgentChatError

ExternalAdapterName = Literal["codex", "claude_code"]

ADAPTER_LABELS: dict[ExternalAdapterName, str] = {
    "codex": "Codex CLI",
    "claude_code": "Claude Code",
}
DEFAULT_EXECUTABLES: dict[ExternalAdapterName, str] = {
    "codex": "codex",
    "claude_code": "claude",
}
DEFAULT_TIMEOUT_SECONDS = 3600
DEFAULT_MAX_TURNS = 20
MAX_TIMEOUT_SECONDS = 6 * 60 * 60
MAX_TURNS = 100


@dataclass(frozen=True, slots=True)
class ExternalRuntimeConfig:
    adapter: ExternalAdapterName
    executable: str | None = None
    timeout_seconds: int = DEFAULT_TIMEOUT_SECONDS
    max_turns: int = DEFAULT_MAX_TURNS


@dataclass(frozen=True, slots=True)
class AdapterCommand:
    argv: list[str]
    redacted_argv: list[str]


@dataclass(frozen=True, slots=True)
class ExternalAdapterStatus:
    adapter: ExternalAdapterName
    label: str
    executable: str
    configured_path: str | None
    resolved_path: str | None
    available: bool
    version: str | None = None
    error: str | None = None


def normalize_external_runtime(raw: dict[str, Any] | None) -> ExternalRuntimeConfig:
    if not isinstance(raw, dict):
        raise AgentChatError("external runtime config is required for external CLI agents")
    adapter = raw.get("adapter")
    if adapter not in ADAPTER_LABELS:
        raise AgentChatError("external runtime adapter must be codex or claude_code")
    executable = raw.get("executable")
    if executable is not None:
        executable = str(executable).strip() or None
    timeout_seconds = int(raw.get("timeout_seconds") or DEFAULT_TIMEOUT_SECONDS)
    if timeout_seconds < 1 or timeout_seconds > MAX_TIMEOUT_SECONDS:
        raise AgentChatError("external runtime timeout_seconds is out of range")
    max_turns = int(raw.get("max_turns") or DEFAULT_MAX_TURNS)
    if max_turns < 1 or max_turns > MAX_TURNS:
        raise AgentChatError("external runtime max_turns is out of range")
    return ExternalRuntimeConfig(
        adapter=adapter,
        executable=executable,
        timeout_seconds=timeout_seconds,
        max_turns=max_turns,
    )


def resolve_executable(config: ExternalRuntimeConfig) -> str:
    executable = config.executable or DEFAULT_EXECUTABLES[config.adapter]
    if any(sep in executable for sep in ("\n", "\r", "\x00")):
        raise AgentChatError("external runtime executable path is invalid")
    resolved = shutil.which(executable)
    if resolved is None:
        raise AgentChatError(
            f"{ADAPTER_LABELS[config.adapter]} executable was not found: {executable}"
        )
    return resolved


def build_command(config: ExternalRuntimeConfig, prompt: str) -> AdapterCommand:
    executable = resolve_executable(config)
    if config.adapter == "codex":
        argv = [executable, "exec", "--sandbox", "danger-full-access", prompt]
    elif config.adapter == "claude_code":
        argv = [
            executable,
            "-p",
            "--output-format",
            "stream-json",
            "--permission-mode",
            "bypassPermissions",
            "--max-turns",
            str(config.max_turns),
            prompt,
        ]
    else:
        raise AgentChatError("unsupported external runtime adapter")
    return AdapterCommand(argv=argv, redacted_argv=[*argv[:-1], "<prompt>"])


def parse_stdout_line(adapter: ExternalAdapterName, line: str) -> list[str]:
    if adapter == "codex":
        return [line]
    if adapter != "claude_code":
        return [line]
    try:
        payload = json.loads(line)
    except json.JSONDecodeError:
        return [line]
    if not isinstance(payload, dict):
        return []
    extracted = _extract_claude_text(payload)
    return [text for text in extracted if text]


def _extract_claude_text(payload: dict[str, Any]) -> list[str]:
    event_type = str(payload.get("type") or "")
    if event_type in {"content_block_delta", "message_delta"}:
        delta = payload.get("delta")
        if isinstance(delta, dict) and isinstance(delta.get("text"), str):
            return [delta["text"]]
    if event_type == "assistant":
        message = payload.get("message")
        if isinstance(message, dict):
            return _extract_content_text(message.get("content"))
    if event_type == "result" and isinstance(payload.get("result"), str):
        return [payload["result"]]
    if isinstance(payload.get("text"), str):
        return [payload["text"]]
    return []


def _extract_content_text(content: Any) -> list[str]:
    if isinstance(content, str):
        return [content]
    if not isinstance(content, list):
        return []
    out: list[str] = []
    for item in content:
        if (
            isinstance(item, dict)
            and item.get("type") == "text"
            and isinstance(item.get("text"), str)
        ):
            out.append(item["text"])
    return out


async def detect_adapter_status(
    adapter: ExternalAdapterName,
    executable_override: str | None = None,
) -> ExternalAdapterStatus:
    configured = executable_override.strip() if executable_override else None
    executable = configured or DEFAULT_EXECUTABLES[adapter]
    if any(sep in executable for sep in ("\n", "\r", "\x00")):
        return ExternalAdapterStatus(
            adapter=adapter,
            label=ADAPTER_LABELS[adapter],
            executable=executable,
            configured_path=configured,
            resolved_path=None,
            available=False,
            error="invalid executable path",
        )
    resolved = shutil.which(executable)
    if resolved is None:
        return ExternalAdapterStatus(
            adapter=adapter,
            label=ADAPTER_LABELS[adapter],
            executable=executable,
            configured_path=configured,
            resolved_path=None,
            available=False,
            error="executable not found",
        )
    try:
        proc = await asyncio.to_thread(
            subprocess.run,
            [resolved, "--version"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=5,
            check=False,
        )
    except Exception as exc:
        return ExternalAdapterStatus(
            adapter=adapter,
            label=ADAPTER_LABELS[adapter],
            executable=executable,
            configured_path=configured,
            resolved_path=resolved,
            available=False,
            error=str(exc),
        )
    version_text = (proc.stdout or proc.stderr).strip()
    return ExternalAdapterStatus(
        adapter=adapter,
        label=ADAPTER_LABELS[adapter],
        executable=executable,
        configured_path=configured,
        resolved_path=resolved,
        available=proc.returncode == 0,
        version=version_text or None,
        error=None if proc.returncode == 0 else version_text or f"exit code {proc.returncode}",
    )
