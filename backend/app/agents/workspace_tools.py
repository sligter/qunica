"""Safe workspace and network tools for provider-native tool calls."""

from __future__ import annotations

import re
import shlex
import subprocess
from pathlib import Path, PurePosixPath, PureWindowsPath
from typing import Any
from urllib.parse import urlparse

import httpx
from langchain_core.tools import BaseTool, tool

from app.agents.context import AgentInvocationContext

MAX_READ_LINES = 2000
MAX_GLOB_RESULTS = 200
MAX_GREP_RESULTS = 200
MAX_FILE_BYTES = 1_000_000
MAX_WRITE_BYTES = 1_000_000
MAX_BASH_TIMEOUT_SECONDS = 10
MAX_BASH_OUTPUT_CHARS = 12_000
MAX_FETCH_BYTES = 500_000
MAX_FETCH_CHARS = 20_000
FETCH_TIMEOUT_SECONDS = 10
EXECUTABLE_TOOL_NAMES = frozenset({"Read", "Write", "Edit", "Glob", "Grep", "Bash", "Fetch"})


class WorkspaceToolError(ValueError):
    """Raised when a workspace tool request is invalid or outside the workspace."""


def _workspace_root(context: AgentInvocationContext) -> Path | None:
    workspace = context.workspace
    if workspace is None or workspace.backend_type != "local" or not workspace.local_path:
        return None
    root = Path(workspace.local_path).expanduser().resolve()
    if not root.exists() or not root.is_dir():
        return None
    return root


def _reject_unsafe_relative_path(value: str) -> Path:
    if not value or not value.strip():
        raise WorkspaceToolError("path must be a non-empty relative path")
    posix_candidate = PurePosixPath(value)
    windows_candidate = PureWindowsPath(value)
    if posix_candidate.is_absolute() or windows_candidate.is_absolute() or windows_candidate.drive:
        raise WorkspaceToolError("path must be relative to the workspace root")
    if "~" in (*posix_candidate.parts, *windows_candidate.parts):
        raise WorkspaceToolError("path must not use home-directory expansion")
    if any(part == ".." for part in (*posix_candidate.parts, *windows_candidate.parts)):
        raise WorkspaceToolError("path must stay inside the workspace root")
    return Path(value)


def _resolve_inside(root: Path, value: str) -> Path:
    relative = _reject_unsafe_relative_path(value)
    resolved = (root / relative).resolve()
    if not resolved.is_relative_to(root):
        raise WorkspaceToolError("path must stay inside the workspace root")
    return resolved


def _validate_glob_pattern(pattern: str) -> str:
    path = _reject_unsafe_relative_path(pattern)
    if any(part == ".." for part in path.parts):
        raise WorkspaceToolError("pattern must stay inside the workspace root")
    return pattern or "**/*"


def _line_numbered(text: str, start_line: int = 1) -> str:
    return "\n".join(
        f"{line_number}\t{line}"
        for line_number, line in enumerate(text.splitlines(), start=start_line)
    )


def _read_file(root: Path, file_path: str, start_line: int = 1, limit: int = MAX_READ_LINES) -> str:
    if start_line < 1:
        raise WorkspaceToolError("start_line must be >= 1")
    if limit < 1:
        raise WorkspaceToolError("limit must be >= 1")
    target = _resolve_inside(root, file_path)
    if not target.exists() or not target.is_file():
        raise WorkspaceToolError("file does not exist")
    if target.stat().st_size > MAX_FILE_BYTES:
        raise WorkspaceToolError("file is too large to read with this tool")
    lines = target.read_text(encoding="utf-8", errors="replace").splitlines()
    selected = lines[start_line - 1 : start_line - 1 + min(limit, MAX_READ_LINES)]
    return _line_numbered("\n".join(selected), start_line=start_line)


def _write_file(root: Path, file_path: str, content: str) -> str:
    encoded = content.encode("utf-8")
    if len(encoded) > MAX_WRITE_BYTES:
        raise WorkspaceToolError("content is too large to write with this tool")
    target = _resolve_inside(root, file_path)
    parent = target.parent.resolve()
    if not parent.is_relative_to(root):
        raise WorkspaceToolError("path must stay inside the workspace root")
    if target.exists() and not target.is_file():
        raise WorkspaceToolError("target path is not a file")
    parent.mkdir(parents=True, exist_ok=True)
    target.write_text(content, encoding="utf-8")
    return f"Wrote {len(encoded)} bytes to {target.relative_to(root).as_posix()}."


def _edit_file(
    root: Path,
    file_path: str,
    old_string: str,
    new_string: str,
    replace_all: bool = False,
) -> str:
    if old_string == "":
        raise WorkspaceToolError("old_string must be non-empty")
    target = _resolve_inside(root, file_path)
    if not target.exists() or not target.is_file():
        raise WorkspaceToolError("file does not exist")
    if target.stat().st_size > MAX_FILE_BYTES:
        raise WorkspaceToolError("file is too large to edit with this tool")
    text = target.read_text(encoding="utf-8", errors="replace")
    occurrences = text.count(old_string)
    if occurrences == 0:
        raise WorkspaceToolError("old_string was not found")
    if occurrences > 1 and not replace_all:
        raise WorkspaceToolError(
            "old_string is not unique; set replace_all=true to replace all matches"
        )
    updated = (
        text.replace(old_string, new_string)
        if replace_all
        else text.replace(old_string, new_string, 1)
    )
    encoded = updated.encode("utf-8")
    if len(encoded) > MAX_WRITE_BYTES:
        raise WorkspaceToolError("edited content is too large to write with this tool")
    target.write_text(updated, encoding="utf-8")
    replaced = occurrences if replace_all else 1
    return f"Edited {target.relative_to(root).as_posix()}; replaced {replaced} occurrence(s)."


def _glob_files(root: Path, pattern: str = "**/*", limit: int = MAX_GLOB_RESULTS) -> str:
    if limit < 1:
        raise WorkspaceToolError("limit must be >= 1")
    safe_pattern = _validate_glob_pattern(pattern)
    matches: list[str] = []
    for match in root.glob(safe_pattern):
        resolved = match.resolve()
        if not resolved.is_relative_to(root):
            continue
        if resolved.is_file():
            matches.append(resolved.relative_to(root).as_posix())
    matches = sorted(matches)[: min(limit, MAX_GLOB_RESULTS)]
    return "\n".join(matches) if matches else "No files matched."


def _grep_files(
    root: Path,
    pattern: str,
    path: str = "**/*",
    limit: int = MAX_GREP_RESULTS,
) -> str:
    if limit < 1:
        raise WorkspaceToolError("limit must be >= 1")
    safe_path = _validate_glob_pattern(path)
    try:
        regex = re.compile(pattern)
    except re.error as exc:
        raise WorkspaceToolError(f"invalid regex: {exc}") from exc

    results: list[str] = []
    for match in sorted(root.glob(safe_path), key=lambda p: p.as_posix()):
        resolved = match.resolve()
        if not resolved.is_relative_to(root) or not resolved.is_file():
            continue
        if resolved.stat().st_size > MAX_FILE_BYTES:
            continue
        rel = resolved.relative_to(root).as_posix()
        for line_number, line in enumerate(
            resolved.read_text(encoding="utf-8", errors="replace").splitlines(), start=1
        ):
            if regex.search(line):
                results.append(f"{rel}:{line_number}:{line}")
                if len(results) >= min(limit, MAX_GREP_RESULTS):
                    return "\n".join(results)
    return "No matches found."


_DESTRUCTIVE_COMMAND_PATTERNS = (
    r"(^|[;&|])\s*(?:sudo\s+|command\s+|builtin\s+|env\s+)*(?:[\w./-]*[/\\])?(rm|del|rmdir|format|shutdown|erase|rd)\b",
    r"\b(powershell|pwsh)\b[^\n]*(remove-item|clear-content|stop-computer)\b",
    r"\bgit\s+reset\s+--hard\b",
    r"\bgit\s+clean\b",
    r"\bgit\s+push\b[^\n]*\s--force(?:\b|-with-lease\b)",
)


def _guard_bash_command(command: str, root: Path) -> None:
    if not command.strip():
        raise WorkspaceToolError("command must be non-empty")
    lowered = command.lower()
    for pattern in _DESTRUCTIVE_COMMAND_PATTERNS:
        if re.search(pattern, lowered):
            raise WorkspaceToolError("command is blocked by workspace safety policy")

    try:
        tokens = shlex.split(command, posix=True)
    except ValueError as exc:
        raise WorkspaceToolError(f"command could not be parsed safely: {exc}") from exc
    for index, token in enumerate(tokens):
        if token in {">", ">>", "1>", "1>>", "2>", "2>>"} and index + 1 < len(tokens):
            _resolve_inside(root, tokens[index + 1])
        elif token.startswith((">", ">>", "1>", "1>>", "2>", "2>>")):
            target = token.lstrip("0123456789>")
            if target:
                _resolve_inside(root, target)


def _run_bash(root: Path, command: str, timeout_seconds: int = MAX_BASH_TIMEOUT_SECONDS) -> str:
    if timeout_seconds < 1 or timeout_seconds > MAX_BASH_TIMEOUT_SECONDS:
        raise WorkspaceToolError(
            f"timeout_seconds must be between 1 and {MAX_BASH_TIMEOUT_SECONDS}"
        )
    _guard_bash_command(command, root)
    try:
        completed = subprocess.run(
            command,
            cwd=root,
            shell=True,
            text=True,
            capture_output=True,
            timeout=timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        stdout = _process_output_to_text(exc.stdout)
        stderr = _process_output_to_text(exc.stderr)
        output = (stdout + stderr).strip()
        summary = f"Command timed out after {timeout_seconds}s."
        if output:
            summary = f"{summary}\n{output}"
        return _truncate_output(summary)
    output_parts = [
        f"exit_code={completed.returncode}",
        completed.stdout.strip(),
        completed.stderr.strip(),
    ]
    return _truncate_output("\n".join(part for part in output_parts if part))


def _process_output_to_text(value: str | bytes | None) -> str:
    if value is None:
        return ""
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    return value


def _truncate_output(output: str) -> str:
    if len(output) <= MAX_BASH_OUTPUT_CHARS:
        return output
    return f"{output[:MAX_BASH_OUTPUT_CHARS]}\n[output truncated]"


def _fetch_url(url: str, timeout_seconds: int = FETCH_TIMEOUT_SECONDS) -> str:
    parsed = urlparse(url)
    if parsed.scheme not in {"http", "https"} or not parsed.netloc:
        raise WorkspaceToolError("url must be an http or https URL")
    if timeout_seconds < 1 or timeout_seconds > FETCH_TIMEOUT_SECONDS:
        raise WorkspaceToolError(f"timeout_seconds must be between 1 and {FETCH_TIMEOUT_SECONDS}")
    with (
        httpx.Client(timeout=timeout_seconds, follow_redirects=True) as client,
        client.stream("GET", url) as response,
    ):
        response.raise_for_status()
        content_type = response.headers.get("content-type", "")
        if content_type and not (
            content_type.startswith("text/")
            or "json" in content_type
            or "xml" in content_type
            or "html" in content_type
        ):
            raise WorkspaceToolError("fetch only supports text-like responses")
        chunks: list[bytes] = []
        bytes_seen = 0
        truncated_bytes = False
        for chunk in response.iter_bytes():
            if not chunk:
                continue
            bytes_seen += len(chunk)
            remaining = MAX_FETCH_BYTES - sum(len(part) for part in chunks)
            if remaining > 0:
                chunks.append(chunk[:remaining])
            if bytes_seen > MAX_FETCH_BYTES:
                truncated_bytes = True
                break
        content = b"".join(chunks)
        response_url = response.url
        status_code = response.status_code
        encoding = response.encoding or "utf-8"
    text = content.decode(encoding, errors="replace")
    snippet = " ".join(text.split())[:MAX_FETCH_CHARS]
    suffix = "\n[response truncated]" if truncated_bytes or len(text) > MAX_FETCH_CHARS else ""
    content_type_label = content_type or "unknown content-type"
    header = f"Fetched {response_url} ({status_code}, {content_type_label})."
    return f"{header}\n{snippet}{suffix}"


def build_workspace_tools(context: AgentInvocationContext) -> dict[str, BaseTool]:
    """Return executable workspace and network tools allowed by the invocation context."""

    root = _workspace_root(context)
    enabled = set(context.executable_tools)
    tools: dict[str, BaseTool] = {}

    if "Fetch" in enabled:

        @tool("Fetch")
        def fetch(url: str, timeout_seconds: int = FETCH_TIMEOUT_SECONDS) -> str:
            """Fetch a bounded text snippet from an http or https URL."""

            return _fetch_url(url, timeout_seconds=timeout_seconds)

        tools["Fetch"] = fetch

    if root is None:
        return tools

    if "Read" in enabled:

        @tool("Read")
        def read(file_path: str, start_line: int = 1, limit: int = MAX_READ_LINES) -> str:
            """Read a workspace file with line numbers. The file path must be relative."""

            return _read_file(root, file_path, start_line=start_line, limit=limit)

        tools["Read"] = read

    if "Write" in enabled:

        @tool("Write")
        def write(file_path: str, content: str) -> str:
            """Create or replace a UTF-8 file under the workspace root."""

            return _write_file(root, file_path, content)

        tools["Write"] = write

    if "Edit" in enabled:

        @tool("Edit")
        def edit(
            file_path: str,
            old_string: str,
            new_string: str,
            replace_all: bool = False,
        ) -> str:
            """Replace exact text in an existing UTF-8 workspace file."""

            return _edit_file(root, file_path, old_string, new_string, replace_all=replace_all)

        tools["Edit"] = edit

    if "Glob" in enabled:

        @tool("Glob")
        def glob(pattern: str = "**/*", limit: int = MAX_GLOB_RESULTS) -> str:
            """List files in the workspace matching a glob pattern."""

            return _glob_files(root, pattern=pattern, limit=limit)

        tools["Glob"] = glob

    if "Grep" in enabled:

        @tool("Grep")
        def grep(pattern: str, path: str = "**/*", limit: int = MAX_GREP_RESULTS) -> str:
            """Search workspace file contents with a regular expression."""

            return _grep_files(root, pattern=pattern, path=path, limit=limit)

        tools["Grep"] = grep

    if "Bash" in enabled:

        @tool("Bash")
        def bash(command: str, timeout_seconds: int = MAX_BASH_TIMEOUT_SECONDS) -> str:
            """Run a guarded shell command in the workspace root with bounded output."""

            return _run_bash(root, command, timeout_seconds=timeout_seconds)

        tools["Bash"] = bash

    return tools


def execute_workspace_tool(tools: dict[str, BaseTool], name: str, args: dict[str, Any]) -> str:
    executor = tools.get(name)
    if executor is None:
        return f"Tool {name!r} is unavailable in this runtime."
    try:
        result = executor.invoke(args)
    except Exception as exc:  # tool errors must be returned to the model, not crash fan-out
        return f"Tool {name!r} failed: {exc}"
    return str(result)


def bind_workspace_tools(chat_model: Any, tools: dict[str, BaseTool]) -> Any:
    """Bind provider-native tools when the chat model supports LangChain binding."""

    if not tools or not hasattr(chat_model, "bind_tools"):
        return chat_model
    try:
        return chat_model.bind_tools(list(tools.values()))
    except (NotImplementedError, TypeError, ValueError):
        return chat_model
