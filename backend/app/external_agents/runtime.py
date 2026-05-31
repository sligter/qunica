from __future__ import annotations

import asyncio
import os
import signal
import subprocess
import sys
import threading
from collections.abc import AsyncIterator
from contextlib import suppress
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Literal, TextIO
from uuid import UUID

from sqlalchemy.ext.asyncio import AsyncSession

from app.core.exceptions import AgentChatError
from app.external_agents.adapters import (
    ExternalRuntimeConfig,
    build_command,
    parse_stdout_line,
)
from app.models.external_agent_run import ExternalAgentRun

MAX_TAIL_CHARS = 12_000


@dataclass(frozen=True, slots=True)
class ExternalAgentEvent:
    kind: str
    data: str | dict[str, object]


@dataclass(frozen=True, slots=True)
class _ProcessEvent:
    kind: Literal["stdout", "done", "error"]
    value: str | int


class _Tail:
    def __init__(self, limit: int = MAX_TAIL_CHARS) -> None:
        self.limit = limit
        self.value = ""
        self._lock = threading.Lock()

    def append(self, text: str) -> None:
        with self._lock:
            self.value = (self.value + text)[-self.limit :]

    def snapshot(self) -> str:
        with self._lock:
            return self.value


def _kill_process_tree(proc: subprocess.Popen[str]) -> None:
    if proc.poll() is not None:
        return
    if sys.platform == "win32":
        subprocess.run(
            ["taskkill", "/PID", str(proc.pid), "/T", "/F"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
    else:
        try:
            os.killpg(os.getpgid(proc.pid), signal.SIGTERM)
        except ProcessLookupError:
            return
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        if sys.platform == "win32":
            proc.kill()
        else:
            try:
                os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
            except ProcessLookupError:
                return
        proc.wait(timeout=5)


def _read_stderr(stream: TextIO | None, tail: _Tail) -> None:
    if stream is None:
        return
    for line in stream:
        tail.append(line)


def _start_process(argv: list[str], cwd: str) -> subprocess.Popen[str]:
    if sys.platform == "win32":
        return subprocess.Popen(
            argv,
            cwd=cwd,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            bufsize=1,
            shell=False,
            creationflags=getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0),
        )
    return subprocess.Popen(
        argv,
        cwd=cwd,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        bufsize=1,
        shell=False,
        start_new_session=True,
    )


def _run_event(
    run: ExternalAgentRun, status: str, summary: str | None = None
) -> ExternalAgentEvent:
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
    return ExternalAgentEvent(kind="run", data=payload)


async def run_external_agent_stream(
    db: AsyncSession,
    *,
    owner_id: UUID,
    group_id: UUID | None,
    agent_id: UUID,
    thread_id: UUID | None,
    config: ExternalRuntimeConfig,
    cwd: Path,
    prompt: str,
) -> AsyncIterator[ExternalAgentEvent]:
    if not cwd.exists() or not cwd.is_dir():
        raise AgentChatError("external agent workspace must be an existing local directory")
    command = build_command(config, prompt)
    run = ExternalAgentRun(
        owner_id=owner_id,
        group_id=group_id,
        agent_id=agent_id,
        thread_id=thread_id,
        adapter=config.adapter,
        cwd=str(cwd.resolve()),
        status="running",
        argv=command.redacted_argv,
    )
    db.add(run)
    await db.flush()
    await db.refresh(run)
    yield _run_event(run, "running")

    stdout_tail = _Tail()
    stderr_tail = _Tail()
    queue: asyncio.Queue[_ProcessEvent] = asyncio.Queue()
    loop = asyncio.get_running_loop()
    proc_holder: dict[str, subprocess.Popen[str]] = {}
    proc_lock = threading.Lock()
    kill_requested = threading.Event()
    status = "completed"
    error_message: str | None = None
    was_cancelled = False
    cwd_text = str(cwd.resolve())

    def send_event(event: _ProcessEvent) -> None:
        with suppress(RuntimeError):
            loop.call_soon_threadsafe(queue.put_nowait, event)

    def current_process() -> subprocess.Popen[str] | None:
        with proc_lock:
            return proc_holder.get("proc")

    def request_kill() -> None:
        kill_requested.set()
        proc = current_process()
        if proc is not None:
            _kill_process_tree(proc)

    def process_runner() -> None:
        stderr_thread: threading.Thread | None = None
        proc: subprocess.Popen[str] | None = None
        try:
            proc = _start_process(command.argv, cwd_text)
            with proc_lock:
                proc_holder["proc"] = proc
            if kill_requested.is_set():
                _kill_process_tree(proc)
            stderr_thread = threading.Thread(
                target=_read_stderr,
                args=(proc.stderr, stderr_tail),
                daemon=True,
            )
            stderr_thread.start()
            if proc.stdout is not None:
                for line in proc.stdout:
                    stdout_tail.append(line)
                    send_event(_ProcessEvent(kind="stdout", value=line))
            exit_code = proc.wait()
            if stderr_thread.is_alive():
                stderr_thread.join(timeout=1)
            send_event(_ProcessEvent(kind="done", value=exit_code))
        except Exception as exc:
            send_event(_ProcessEvent(kind="error", value=str(exc)))
        finally:
            if stderr_thread is not None and stderr_thread.is_alive():
                stderr_thread.join(timeout=1)

    try:
        runner_thread = threading.Thread(target=process_runner, daemon=True)
        runner_thread.start()
        async with asyncio.timeout(config.timeout_seconds):
            while True:
                process_event = await queue.get()
                if process_event.kind == "stdout" and isinstance(process_event.value, str):
                    for token in parse_stdout_line(config.adapter, process_event.value):
                        yield ExternalAgentEvent(kind="token", data=token)
                elif process_event.kind == "done" and isinstance(process_event.value, int):
                    run.exit_code = process_event.value
                    if process_event.value != 0:
                        status = "failed"
                        error_message = (
                            stderr_tail.snapshot().strip()
                            or f"external agent exited with code {process_event.value}"
                        )
                    break
                elif process_event.kind == "error" and isinstance(process_event.value, str):
                    status = "failed"
                    error_message = process_event.value
                    break
    except TimeoutError:
        status = "timeout"
        error_message = f"external agent timed out after {config.timeout_seconds} seconds"
        request_kill()
        proc = current_process()
        if proc is not None:
            run.exit_code = proc.returncode
    except asyncio.CancelledError:
        status = "cancelled"
        was_cancelled = True
        error_message = "external agent run was cancelled"
        request_kill()
        proc = current_process()
        if proc is not None:
            run.exit_code = proc.returncode
        raise
    except Exception as exc:
        status = "failed"
        error_message = str(exc)
        request_kill()
        proc = current_process()
        if proc is not None:
            run.exit_code = proc.returncode
    finally:
        run.status = status
        run.error_message = error_message
        run.stdout_tail = stdout_tail.snapshot() or None
        run.stderr_tail = stderr_tail.snapshot() or None
        run.ended_at = datetime.now(UTC)
        await db.flush()
        await db.refresh(run)
        if not was_cancelled:
            yield _run_event(run, status, error_message)

    if status != "completed":
        raise AgentChatError(error_message or "external agent failed")


async def run_external_agent(
    db: AsyncSession,
    *,
    owner_id: UUID,
    group_id: UUID | None,
    agent_id: UUID,
    thread_id: UUID | None,
    config: ExternalRuntimeConfig,
    cwd: Path,
    prompt: str,
) -> str:
    chunks: list[str] = []
    async for event in run_external_agent_stream(
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
