"""Safe read-only workspace tools for provider-native tool calls."""

from __future__ import annotations

import re
from pathlib import Path, PurePosixPath, PureWindowsPath
from typing import Any

from langchain_core.tools import BaseTool, tool

from app.agents.context import AgentInvocationContext

MAX_READ_LINES = 2000
MAX_GLOB_RESULTS = 200
MAX_GREP_RESULTS = 200
MAX_FILE_BYTES = 1_000_000
EXECUTABLE_TOOL_NAMES = frozenset({"Read", "Glob", "Grep"})


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
    posix_candidate = PurePosixPath(value)
    windows_candidate = PureWindowsPath(value)
    if posix_candidate.is_absolute() or windows_candidate.is_absolute():
        raise WorkspaceToolError("path must be relative to the workspace root")
    if any(part == ".." for part in (*posix_candidate.parts, *windows_candidate.parts)):
        raise WorkspaceToolError("path must stay inside the workspace root")
    return Path(value).expanduser()


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


def build_workspace_tools(context: AgentInvocationContext) -> dict[str, BaseTool]:
    """Return executable read-only tools allowed by the invocation context."""

    root = _workspace_root(context)
    if root is None:
        return {}
    enabled = set(context.executable_tools)
    tools: dict[str, BaseTool] = {}

    if "Read" in enabled:

        @tool("Read")
        def read(file_path: str, start_line: int = 1, limit: int = MAX_READ_LINES) -> str:
            """Read a workspace file with line numbers. The file path must be relative."""

            return _read_file(root, file_path, start_line=start_line, limit=limit)

        tools["Read"] = read

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
