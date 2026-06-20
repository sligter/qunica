from __future__ import annotations

import asyncio
import asyncio.subprocess as aio_subprocess
import contextlib
import json
import os
import subprocess
import sys
import tempfile
import threading
from collections.abc import AsyncIterator, Awaitable, Callable
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, cast
from uuid import UUID, uuid4

from acp import PROTOCOL_VERSION, Client, RequestError, text_block
from acp.core import ClientSideConnection
from acp.schema import (
    AgentMessageChunk,
    AgentPlanUpdate,
    AgentThoughtChunk,
    AllowedOutcome,
    AvailableCommandsUpdate,
    ClientCapabilities,
    ConfigOptionUpdate,
    CreateTerminalResponse,
    CurrentModeUpdate,
    DeniedOutcome,
    EmbeddedResourceContentBlock,
    EnvVariable,
    ImageContentBlock,
    Implementation,
    KillTerminalResponse,
    PermissionOption,
    ReadTextFileResponse,
    ReleaseTerminalResponse,
    RequestPermissionResponse,
    ResourceContentBlock,
    SessionInfoUpdate,
    TerminalOutputResponse,
    TextContentBlock,
    ToolCallProgress,
    ToolCallStart,
    ToolCallUpdate,
    UsageUpdate,
    UserMessageChunk,
    WaitForTerminalExitResponse,
    WriteTextFileResponse,
)
from acp.transports import default_environment
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.exceptions import AgentChatError
from app.external_agents.adapters import AcpConfigValue, AcpRuntimeConfig, PermissionPolicy
from app.models.external_agent_run import ExternalAgentRun

MAX_TAIL_CHARS = 12_000
MAX_METADATA_CHARS = 1_000
ACP_READ_CHUNK_BYTES = 64 * 1024


@dataclass(frozen=True, slots=True)
class AcpAgentEvent:
    kind: str
    data: str | dict[str, object]


@dataclass(frozen=True, slots=True)
class _AcpTurnResult:
    status: str
    error_message: str | None
    exit_code: int | None
    stdout_tail: str | None
    stderr_tail: str | None
    was_cancelled: bool = False


@dataclass(frozen=True, slots=True)
class _AcpDone:
    result: _AcpTurnResult


_AcpQueueItem = AcpAgentEvent | _AcpDone


class _Tail:
    def __init__(self, limit: int = MAX_TAIL_CHARS) -> None:
        self.limit = limit
        self.value = ""

    def append(self, text: str) -> None:
        self.value = (self.value + text)[-self.limit :]

    def snapshot(self) -> str:
        return self.value


class _UnboundedLineStreamReader(asyncio.StreamReader):
    def __init__(self, source: asyncio.StreamReader) -> None:
        super().__init__()
        self._source = source
        self._pending = bytearray()

    async def readline(self) -> bytes:
        while True:
            newline_index = self._pending.find(b"\n")
            if newline_index >= 0:
                line = bytes(self._pending[: newline_index + 1])
                del self._pending[: newline_index + 1]
                return line

            chunk = await self._source.read(ACP_READ_CHUNK_BYTES)
            if not chunk:
                if self._pending:
                    line = bytes(self._pending)
                    self._pending.clear()
                    return line
                return b""

            self._pending.extend(chunk)


class _AgSwarmerAcpClient(Client):
    def __init__(self, permission_policy: PermissionPolicy) -> None:
        self.permission_policy = permission_policy
        self.events: asyncio.Queue[AcpAgentEvent] = asyncio.Queue()

    def on_connect(self, conn: object) -> None:
        _ = conn

    async def request_permission(
        self,
        options: list[PermissionOption],
        session_id: str,
        tool_call: ToolCallUpdate,
        **kwargs: Any,
    ) -> RequestPermissionResponse:
        _ = (session_id, tool_call, kwargs)
        if self.permission_policy == "auto_allow":
            selected = _first_allow_option(options)
            if selected is not None:
                return RequestPermissionResponse(
                    outcome=AllowedOutcome(
                        option_id=selected.option_id,
                        outcome="selected",
                    )
                )
        return RequestPermissionResponse(outcome=DeniedOutcome(outcome="cancelled"))

    async def write_text_file(
        self,
        content: str,
        path: str,
        session_id: str,
        **kwargs: Any,
    ) -> WriteTextFileResponse | None:
        _ = (content, path, session_id, kwargs)
        raise RequestError.method_not_found("fs/write_text_file")

    async def read_text_file(
        self,
        path: str,
        session_id: str,
        limit: int | None = None,
        line: int | None = None,
        **kwargs: Any,
    ) -> ReadTextFileResponse:
        _ = (path, session_id, limit, line, kwargs)
        raise RequestError.method_not_found("fs/read_text_file")

    async def create_terminal(
        self,
        command: str,
        session_id: str,
        args: list[str] | None = None,
        cwd: str | None = None,
        env: list[EnvVariable] | None = None,
        output_byte_limit: int | None = None,
        **kwargs: Any,
    ) -> CreateTerminalResponse:
        _ = (command, session_id, args, cwd, env, output_byte_limit, kwargs)
        raise RequestError.method_not_found("terminal/create")

    async def terminal_output(
        self,
        session_id: str,
        terminal_id: str,
        **kwargs: Any,
    ) -> TerminalOutputResponse:
        _ = (session_id, terminal_id, kwargs)
        raise RequestError.method_not_found("terminal/output")

    async def release_terminal(
        self,
        session_id: str,
        terminal_id: str,
        **kwargs: Any,
    ) -> ReleaseTerminalResponse | None:
        _ = (session_id, terminal_id, kwargs)
        raise RequestError.method_not_found("terminal/release")

    async def wait_for_terminal_exit(
        self,
        session_id: str,
        terminal_id: str,
        **kwargs: Any,
    ) -> WaitForTerminalExitResponse:
        _ = (session_id, terminal_id, kwargs)
        raise RequestError.method_not_found("terminal/wait_for_exit")

    async def kill_terminal(
        self,
        session_id: str,
        terminal_id: str,
        **kwargs: Any,
    ) -> KillTerminalResponse | None:
        _ = (session_id, terminal_id, kwargs)
        raise RequestError.method_not_found("terminal/kill")

    async def session_update(
        self,
        session_id: str,
        update: (
            UserMessageChunk
            | AgentMessageChunk
            | AgentThoughtChunk
            | ToolCallStart
            | ToolCallProgress
            | AgentPlanUpdate
            | AvailableCommandsUpdate
            | CurrentModeUpdate
            | ConfigOptionUpdate
            | SessionInfoUpdate
            | UsageUpdate
        ),
        **kwargs: Any,
    ) -> None:
        _ = (session_id, kwargs)
        event = _event_from_update(update)
        if event is not None:
            self.events.put_nowait(event)

    async def ext_method(self, method: str, params: dict[str, Any]) -> dict[str, Any]:
        _ = params
        raise RequestError.method_not_found(method)

    async def ext_notification(self, method: str, params: dict[str, Any]) -> None:
        _ = params
        raise RequestError.method_not_found(method)


def _first_allow_option(options: list[PermissionOption]) -> PermissionOption | None:
    for option in options:
        if option.kind in {"allow_once", "allow_always"}:
            return option
    return None


async def _read_stderr(stream: asyncio.StreamReader | None, tail: _Tail) -> None:
    if stream is None:
        return
    while True:
        line = await stream.readline()
        if not line:
            return
        tail.append(line.decode("utf-8", errors="replace"))


def _host_cli_auth_env(profile: str) -> dict[str, str]:
    if profile not in {"codex", "claude"}:
        return {}
    env_keys = [
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
    ]
    if profile == "codex":
        env_keys.append("CODEX_HOME")
    if profile == "claude":
        env_keys.extend(["CLAUDE_CONFIG_DIR", "CLAUDE_HOME", "ANTHROPIC_MODEL"])
    return {
        key: value
        for key in env_keys
        if (value := os.environ.get(key))
    }


def _acp_agent_env(
    isolated_home: Path,
    runtime_env: dict[str, str],
    *,
    profile: str = "custom",
) -> dict[str, str]:
    if profile in {"codex", "claude"}:
        return {
            "AG_SWARMER_ACP_AGENT": "1",
            **_host_cli_auth_env(profile),
            **runtime_env,
        }

    isolated_home.mkdir(parents=True, exist_ok=True)
    config_dir = isolated_home / "config"
    data_dir = isolated_home / "data"
    cache_dir = isolated_home / "cache"
    for path in (config_dir, data_dir, cache_dir):
        path.mkdir(parents=True, exist_ok=True)

    env = {
        "AG_SWARMER_ACP_AGENT": "1",
        "HOME": str(isolated_home),
        "USERPROFILE": str(isolated_home),
        "APPDATA": str(config_dir),
        "LOCALAPPDATA": str(data_dir),
        "XDG_CONFIG_HOME": str(config_dir),
        "XDG_DATA_HOME": str(data_dir),
        "XDG_CACHE_HOME": str(cache_dir),
        "CODEX_HOME": str(config_dir / "codex"),
        "CLAUDE_CONFIG_DIR": str(config_dir / "claude"),
        "CLAUDE_HOME": str(config_dir / "claude"),
    }
    env.update(runtime_env)
    return env


def _windows_hidden_subprocess_kwargs() -> dict[str, Any]:
    if sys.platform != "win32":
        return {}
    startupinfo = subprocess.STARTUPINFO()
    startupinfo.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    startupinfo.wShowWindow = subprocess.SW_HIDE
    return {
        "creationflags": getattr(subprocess, "CREATE_NO_WINDOW", 0),
        "startupinfo": startupinfo,
    }


@contextlib.asynccontextmanager
async def _spawn_hidden_stdio_transport(
    command: str,
    *args: str,
    env: dict[str, str] | None = None,
    cwd: str | Path | None = None,
    stderr: int | None = aio_subprocess.PIPE,
    shutdown_timeout: float = 2.0,
) -> AsyncIterator[
    tuple[asyncio.StreamReader, asyncio.StreamWriter, asyncio.subprocess.Process]
]:
    merged_env = dict(default_environment())
    if env:
        merged_env.update(env)

    process = await asyncio.create_subprocess_exec(
        command,
        *args,
        stdin=aio_subprocess.PIPE,
        stdout=aio_subprocess.PIPE,
        stderr=stderr,
        env=merged_env,
        cwd=str(cwd) if cwd is not None else None,
        **_windows_hidden_subprocess_kwargs(),
    )

    if process.stdout is None or process.stdin is None:
        process.kill()
        await process.wait()
        raise RuntimeError("ACP stdio transport requires stdout/stdin pipes")

    try:
        yield process.stdout, process.stdin, process
    finally:
        if process.stdin is not None:
            try:
                process.stdin.write_eof()
            except (AttributeError, OSError, RuntimeError):
                process.stdin.close()
            with contextlib.suppress(Exception):
                await process.stdin.drain()
            with contextlib.suppress(Exception):
                process.stdin.close()
            with contextlib.suppress(Exception):
                await process.stdin.wait_closed()

        try:
            await asyncio.wait_for(process.wait(), timeout=shutdown_timeout)
        except TimeoutError:
            process.terminate()
            try:
                await asyncio.wait_for(process.wait(), timeout=shutdown_timeout)
            except TimeoutError:
                process.kill()
                await process.wait()


@contextlib.asynccontextmanager
async def _spawn_acp_agent_process(
    to_client: Client,
    command: str,
    *args: str,
    env: dict[str, str] | None = None,
    cwd: str | Path | None = None,
) -> AsyncIterator[tuple[Any, asyncio.subprocess.Process]]:
    async with _spawn_hidden_stdio_transport(command, *args, env=env, cwd=cwd) as (
        reader,
        writer,
        process,
    ):
        conn = ClientSideConnection(to_client, writer, _UnboundedLineStreamReader(reader))
        try:
            yield conn, process
        finally:
            await conn.close()


def _run_event(
    run: ExternalAgentRun, status: str, summary: str | None = None
) -> AcpAgentEvent:
    payload: dict[str, object] = {
        "run_id": str(run.id),
        "agent_id": str(run.agent_id),
        "adapter": run.adapter,
        "status": status,
        "cwd": run.cwd,
    }
    if run.exit_code is not None:
        payload["exit_code"] = run.exit_code
    if summary:
        payload["summary"] = summary
    return AcpAgentEvent(kind="run", data=payload)


def _event_from_update(update: object) -> AcpAgentEvent | None:
    if isinstance(update, AgentMessageChunk):
        text = _content_text(update.content)
        return AcpAgentEvent(kind="token", data=text) if text else None
    if isinstance(update, AgentThoughtChunk):
        text = _content_text(update.content)
        return AcpAgentEvent(kind="reasoning", data=text) if text else None
    if isinstance(update, ToolCallStart):
        return AcpAgentEvent(kind="tool_call_start", data=_tool_start_payload(update))
    if isinstance(update, ToolCallProgress):
        return AcpAgentEvent(kind="tool_call_result", data=_tool_progress_payload(update))
    if isinstance(update, UsageUpdate):
        return AcpAgentEvent(
            kind="usage",
            data={"used": update.used, "size": update.size},
        )
    return None


def _content_text(content: object) -> str:
    if isinstance(content, TextContentBlock):
        return content.text
    if isinstance(content, ResourceContentBlock):
        return content.uri or content.name or ""
    if isinstance(content, EmbeddedResourceContentBlock):
        resource = content.resource
        text = getattr(resource, "text", None)
        return text if isinstance(text, str) else ""
    if isinstance(content, ImageContentBlock):
        return ""
    if isinstance(content, dict):
        text = content.get("text")
        return text if isinstance(text, str) else ""
    return ""


def _tool_start_payload(update: ToolCallStart) -> dict[str, object]:
    status = update.status or "started"
    return {
        "tool_call_id": update.tool_call_id,
        "tool_name": update.title,
        "status": "started" if status in {"pending", "in_progress"} else status,
        "args_summary": _bounded_metadata(update.raw_input or update.kind or ""),
    }


def _tool_progress_payload(update: ToolCallProgress) -> dict[str, object]:
    status = update.status or "completed"
    return {
        "tool_call_id": update.tool_call_id,
        "tool_name": update.title or "ACP tool call",
        "status": status,
        "result_summary": _bounded_metadata(update.raw_output or update.content or ""),
    }


def _bounded_metadata(value: object) -> str:
    if isinstance(value, str):
        text = value
    elif hasattr(value, "model_dump"):
        payload = cast(Any, value).model_dump(mode="json", by_alias=True)
        text = json.dumps(payload, ensure_ascii=False, default=str)
    else:
        text = json.dumps(value, ensure_ascii=False, default=str)
    if len(text) <= MAX_METADATA_CHARS:
        return text
    return f"{text[:MAX_METADATA_CHARS]}..."


async def _cancel_prompt_task(task: asyncio.Task[object] | None) -> None:
    if task is None or task.done():
        return
    task.cancel()
    with contextlib.suppress(asyncio.CancelledError):
        await task


async def _cancel_session(conn: object | None, session_id: str | None) -> None:
    if conn is None or session_id is None:
        return
    cancel = getattr(conn, "cancel", None)
    if cancel is None:
        return
    with contextlib.suppress(Exception):
        await asyncio.wait_for(cancel(session_id=session_id), timeout=2)


async def run_acp_agent_stream(
    db: AsyncSession,
    *,
    owner_id: UUID,
    group_id: UUID | None,
    agent_id: UUID,
    thread_id: UUID | None,
    config: AcpRuntimeConfig,
    cwd: Path,
    prompt: str,
) -> AsyncIterator[AcpAgentEvent]:
    if not cwd.exists() or not cwd.is_dir():
        raise AgentChatError("ACP agent workspace must be an existing local directory")

    run = ExternalAgentRun(
        owner_id=owner_id,
        group_id=group_id,
        agent_id=agent_id,
        thread_id=thread_id,
        adapter="acp",
        cwd=str(cwd.resolve()),
        status="running",
        argv=[config.command, *config.args],
    )
    db.add(run)
    await db.flush()
    await db.refresh(run)
    yield _run_event(run, "running")

    queue: asyncio.Queue[_AcpQueueItem] = asyncio.Queue()
    process_task: asyncio.Task[None] | None = None
    worker_thread: threading.Thread | None = None
    worker_cancel_event: threading.Event | None = None
    result = _AcpTurnResult(
        status="failed",
        error_message="ACP agent did not produce a result",
        exit_code=None,
        stdout_tail=None,
        stderr_tail=None,
    )
    try:
        if _should_run_acp_in_worker_thread():
            worker_thread, worker_cancel_event = _start_acp_worker_thread(
                asyncio.get_running_loop(),
                queue,
                config,
                cwd,
                prompt,
            )
        else:
            process_task = asyncio.create_task(
                _run_acp_process_on_current_loop(queue, config, cwd, prompt)
            )
        while True:
            item = await queue.get()
            if isinstance(item, _AcpDone):
                result = item.result
                break
            yield item
    except asyncio.CancelledError:
        result = _AcpTurnResult(
            status="cancelled",
            error_message="ACP agent run was cancelled",
            exit_code=None,
            stdout_tail=None,
            stderr_tail=None,
            was_cancelled=True,
        )
        if worker_cancel_event is not None:
            worker_cancel_event.set()
        if process_task is not None:
            process_task.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await process_task
        raise
    finally:
        if worker_thread is not None and not worker_thread.is_alive():
            worker_thread.join(timeout=0)
        run.status = result.status
        run.error_message = result.error_message
        run.exit_code = result.exit_code
        run.stdout_tail = result.stdout_tail
        run.stderr_tail = result.stderr_tail
        run.ended_at = datetime.now(UTC)
        await db.flush()
        await db.refresh(run)
        if not result.was_cancelled:
            yield _run_event(run, result.status, result.error_message)

    if result.status != "completed":
        raise AgentChatError(result.error_message or "ACP agent failed")


def _request_error_message(exc: RequestError) -> str:
    payload = exc.to_error_obj()
    code = payload.get("code")
    message = payload.get("message")
    return f"ACP request failed ({code}): {message}"


def _is_method_not_found(exc: RequestError) -> bool:
    return exc.to_error_obj().get("code") == -32601


def _new_session_meta(config: AcpRuntimeConfig) -> dict[str, Any]:
    if config.profile != "claude":
        return {}
    options: dict[str, object] = {}
    if config.thinking_effort:
        options["effortLevel"] = config.thinking_effort
    return {"claudeCode": {"options": options}} if options else {}


async def _apply_session_settings(
    conn: Any,
    session_id: str,
    config: AcpRuntimeConfig,
) -> None:
    if config.model:
        await _apply_session_model(conn, session_id, config.model)
    if config.mode:
        await _apply_session_mode(conn, session_id, config.mode, config.profile)
    if config.thinking_effort:
        await _apply_first_config_option(
            conn,
            session_id,
            _thinking_config_option_ids(config.profile),
            config.thinking_effort,
        )
    for key, value in (config.config_options or {}).items():
        await conn.set_config_option(config_id=key, session_id=session_id, value=value)


async def _apply_session_model(conn: Any, session_id: str, model: str) -> None:
    try:
        await conn.set_session_model(model_id=model, session_id=session_id)
    except RequestError as exc:
        if not _is_method_not_found(exc):
            raise
        await _apply_first_config_option(conn, session_id, ["model"], model)


async def _apply_session_mode(
    conn: Any,
    session_id: str,
    mode: str,
    profile: str,
) -> None:
    try:
        await conn.set_session_mode(mode_id=mode, session_id=session_id)
    except RequestError as exc:
        if not _is_method_not_found(exc):
            raise
        await _apply_first_config_option(conn, session_id, _mode_config_option_ids(profile), mode)


async def _apply_first_config_option(
    conn: Any,
    session_id: str,
    option_ids: list[str],
    value: AcpConfigValue,
) -> None:
    last_error: RequestError | None = None
    for option_id in option_ids:
        try:
            await conn.set_config_option(
                config_id=option_id,
                session_id=session_id,
                value=value,
            )
            return
        except RequestError as exc:
            last_error = exc
    if last_error is not None:
        raise last_error


def _thinking_config_option_ids(profile: str) -> list[str]:
    if profile == "claude":
        return ["effort", "effortLevel", "reasoning_effort"]
    return ["reasoning_effort", "effort", "effortLevel"]


def _mode_config_option_ids(profile: str) -> list[str]:
    if profile == "claude":
        return ["mode", "permissionMode", "permissions.defaultMode"]
    return ["mode", "approval_preset"]


async def _run_acp_process(
    config: AcpRuntimeConfig,
    cwd: Path,
    prompt: str,
    emit: Callable[[AcpAgentEvent], Awaitable[None]],
    cancel_requested: Callable[[], bool] | None = None,
) -> _AcpTurnResult:
    status = "completed"
    error_message: str | None = None
    was_cancelled = False
    stderr_tail = _Tail()
    proc_returncode: int | None = None
    prompt_task: asyncio.Task[object] | None = None
    conn: object | None = None
    session_id: str | None = None
    stderr_task: asyncio.Task[None] | None = None

    try:
        with tempfile.TemporaryDirectory(prefix="ag-swarmer-acp-") as home_text:
            env = _acp_agent_env(Path(home_text), config.env, profile=config.profile)
            client = _AgSwarmerAcpClient(config.permission_policy)
            async with _spawn_acp_agent_process(
                client,
                config.command,
                *config.args,
                env=env,
                cwd=str(cwd.resolve()),
            ) as (active_conn, proc):
                conn = active_conn
                stderr_task = asyncio.create_task(_read_stderr(proc.stderr, stderr_tail))
                try:
                    async with asyncio.timeout(config.timeout_seconds):
                        await active_conn.initialize(
                            protocol_version=PROTOCOL_VERSION,
                            client_capabilities=ClientCapabilities(),
                            client_info=Implementation(
                                name="ag-swarmer",
                                title="AG Swarmer",
                                version="0.1.0",
                            ),
                        )
                        session = await active_conn.new_session(
                            cwd=str(cwd.resolve()),
                            mcp_servers=[],
                            **_new_session_meta(config),
                        )
                        session_id = session.session_id
                        await _apply_session_settings(active_conn, session_id, config)
                        prompt_task = asyncio.create_task(
                            active_conn.prompt(
                                session_id=session_id,
                                prompt=[text_block(prompt)],
                                message_id=str(uuid4()),
                            )
                        )
                        while not prompt_task.done():
                            if cancel_requested is not None and cancel_requested():
                                status = "cancelled"
                                was_cancelled = True
                                error_message = "ACP agent run was cancelled"
                                await _cancel_session(active_conn, session_id)
                                await _cancel_prompt_task(prompt_task)
                                break
                            try:
                                event = await asyncio.wait_for(client.events.get(), timeout=0.1)
                            except TimeoutError:
                                continue
                            await emit(event)
                        if prompt_task.done() and not prompt_task.cancelled():
                            response = await prompt_task
                            while not client.events.empty():
                                await emit(client.events.get_nowait())
                            stop_reason = getattr(response, "stop_reason", None)
                            if stop_reason == "cancelled":
                                status = "cancelled"
                                error_message = "ACP agent cancelled the turn"
                except TimeoutError:
                    status = "timeout"
                    error_message = f"ACP agent timed out after {config.timeout_seconds} seconds"
                    await _cancel_session(active_conn, session_id)
                    await _cancel_prompt_task(prompt_task)
                except asyncio.CancelledError:
                    status = "cancelled"
                    was_cancelled = True
                    error_message = "ACP agent run was cancelled"
                    await _cancel_session(active_conn, session_id)
                    await _cancel_prompt_task(prompt_task)
                    raise
                except RequestError as exc:
                    status = "failed"
                    error_message = _request_error_message(exc)
                    await _cancel_prompt_task(prompt_task)
                except Exception as exc:
                    status = "failed"
                    error_message = str(exc) or exc.__class__.__name__
                    await _cancel_prompt_task(prompt_task)
                finally:
                    proc_returncode = proc.returncode
            if proc_returncode is None:
                proc_returncode = proc.returncode
            if stderr_task is not None:
                with contextlib.suppress(Exception):
                    await asyncio.wait_for(stderr_task, timeout=2)
    except asyncio.CancelledError:
        raise
    except Exception as exc:
        status = "failed"
        error_message = str(exc) or exc.__class__.__name__
        await _cancel_session(conn, session_id)
        await _cancel_prompt_task(prompt_task)

    return _AcpTurnResult(
        status=status,
        error_message=error_message,
        exit_code=proc_returncode,
        stdout_tail=None,
        stderr_tail=stderr_tail.snapshot() or None,
        was_cancelled=was_cancelled,
    )


async def _run_acp_process_on_current_loop(
    queue: asyncio.Queue[_AcpQueueItem],
    config: AcpRuntimeConfig,
    cwd: Path,
    prompt: str,
) -> None:
    async def emit(event: AcpAgentEvent) -> None:
        await queue.put(event)

    try:
        result = await _run_acp_process(config, cwd, prompt, emit)
    except asyncio.CancelledError:
        result = _AcpTurnResult(
            status="cancelled",
            error_message="ACP agent run was cancelled",
            exit_code=None,
            stdout_tail=None,
            stderr_tail=None,
            was_cancelled=True,
        )
        raise
    finally:
        if "result" in locals():
            await queue.put(_AcpDone(result))


def _start_acp_worker_thread(
    main_loop: asyncio.AbstractEventLoop,
    queue: asyncio.Queue[_AcpQueueItem],
    config: AcpRuntimeConfig,
    cwd: Path,
    prompt: str,
) -> tuple[threading.Thread, threading.Event]:
    cancel_event = threading.Event()

    def emit_from_thread(event: AcpAgentEvent) -> None:
        main_loop.call_soon_threadsafe(queue.put_nowait, event)

    async def run_worker() -> None:
        async def emit(event: AcpAgentEvent) -> None:
            emit_from_thread(event)

        try:
            result = await _run_acp_process(
                config,
                cwd,
                prompt,
                emit,
                cancel_requested=cancel_event.is_set,
            )
        except asyncio.CancelledError:
            result = _AcpTurnResult(
                status="cancelled",
                error_message="ACP agent run was cancelled",
                exit_code=None,
                stdout_tail=None,
                stderr_tail=None,
                was_cancelled=True,
            )
        main_loop.call_soon_threadsafe(queue.put_nowait, _AcpDone(result))

    def runner() -> None:
        if sys.platform == "win32":
            loop = asyncio.ProactorEventLoop()
        else:
            loop = asyncio.new_event_loop()
        try:
            asyncio.set_event_loop(loop)
            loop.run_until_complete(run_worker())
        finally:
            with contextlib.suppress(Exception):
                loop.run_until_complete(asyncio.sleep(0.1))
            loop.close()

    thread = threading.Thread(
        target=runner,
        name="ag-swarmer-acp-runtime",
        daemon=True,
    )
    thread.start()
    return thread, cancel_event


def _should_run_acp_in_worker_thread() -> bool:
    return sys.platform == "win32"


async def run_acp_agent(
    db: AsyncSession,
    *,
    owner_id: UUID,
    group_id: UUID | None,
    agent_id: UUID,
    thread_id: UUID | None,
    config: AcpRuntimeConfig,
    cwd: Path,
    prompt: str,
) -> str:
    chunks: list[str] = []
    async for event in run_acp_agent_stream(
        db,
        owner_id=owner_id,
        group_id=group_id,
        agent_id=agent_id,
        thread_id=thread_id,
        config=config,
        cwd=cwd,
        prompt=prompt,
    ):
        if event.kind == "token" and isinstance(event.data, str):
            chunks.append(event.data)
    return "".join(chunks).strip()
