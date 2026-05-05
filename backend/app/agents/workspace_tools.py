"""Safe workspace and network tools for provider-native tool calls."""

from __future__ import annotations

import json
import re
import shlex
import subprocess
from pathlib import Path, PurePosixPath, PureWindowsPath
from typing import Any
from urllib.parse import urlparse

import httpx
from langchain_core.tools import BaseTool, tool

from app.agents.context import AgentInvocationContext
from app.core.config import settings

MAX_READ_LINES = 2000
MAX_GLOB_RESULTS = 200
MAX_GREP_RESULTS = 200
MAX_FILE_BYTES = 1_000_000
MAX_WRITE_BYTES = 1_000_000
MAX_BASH_TIMEOUT_SECONDS = 10
MAX_BASH_OUTPUT_CHARS = 12_000
MAX_FETCH_BYTES = 500_000
MAX_FETCH_CHARS = 20_000
MAX_SEARCH_RESULTS = 5
MAX_SEARCH_QUERY_CHARS = 500
FETCH_TIMEOUT_SECONDS = 10
EXECUTABLE_TOOL_NAMES = frozenset(
    {
        "Read",
        "Write",
        "Edit",
        "Glob",
        "Grep",
        "Bash",
        "AskUser",
        "WebSearch",
        "Fetch",
        "RunSubAgent",
        "AgentAsTool",
        "GenerateImage",
        "GenerateVideo",
        "SkillManager",
        "TodoWrite",
        "ExitPlanMode",
    }
)


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


def _controlled_tool_result(tool_name: str, message: str, status: str = "SETUP_REQUIRED") -> str:
    return json.dumps(
        {"tool": tool_name, "status": status, "message": message},
        ensure_ascii=False,
    )


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


def _web_search(query: str, max_results: int = MAX_SEARCH_RESULTS) -> str:
    if not query.strip():
        raise WorkspaceToolError("query must be non-empty")
    if len(query) > MAX_SEARCH_QUERY_CHARS:
        raise WorkspaceToolError(f"query must be at most {MAX_SEARCH_QUERY_CHARS} characters")
    if max_results < 1 or max_results > MAX_SEARCH_RESULTS:
        raise WorkspaceToolError(f"max_results must be between 1 and {MAX_SEARCH_RESULTS}")
    if settings.tavily_api_key:
        payload = {
            "api_key": settings.tavily_api_key,
            "query": query,
            "max_results": max_results,
            "include_answer": True,
            "include_raw_content": False,
        }
        with httpx.Client(timeout=FETCH_TIMEOUT_SECONDS) as client:
            response = client.post(settings.tavily_search_url, json=payload)
            response.raise_for_status()
            data = response.json()
        answer = str(data.get("answer") or "")[:MAX_FETCH_CHARS]
        results = []
        for item in data.get("results", [])[:max_results]:
            if not isinstance(item, dict):
                continue
            results.append(
                {
                    "title": str(item.get("title") or "")[:300],
                    "url": str(item.get("url") or "")[:1000],
                    "content": str(item.get("content") or "")[:2000],
                }
            )
        return json.dumps(
            {"tool": "WebSearch", "status": "COMPLETED", "answer": answer, "results": results},
            ensure_ascii=False,
        )
    if settings.playwright_search_url:
        with httpx.Client(timeout=FETCH_TIMEOUT_SECONDS, follow_redirects=True) as client:
            response = client.get(
                settings.playwright_search_url,
                params={"q": query, "max_results": max_results},
            )
            response.raise_for_status()
            text = " ".join(response.text.split())[:MAX_FETCH_CHARS]
        return json.dumps(
            {
                "tool": "WebSearch",
                "status": "COMPLETED",
                "provider": "playwright",
                "content": text,
            },
            ensure_ascii=False,
        )
    return _controlled_tool_result(
        "WebSearch",
        "No search provider is configured. Set TAVILY_API_KEY for Tavily or "
        "PLAYWRIGHT_SEARCH_URL for a Playwright-backed search service.",
    )


def build_workspace_tools(
    context: AgentInvocationContext,
    agent_tool_executor: Any | None = None,
) -> dict[str, BaseTool]:
    """Return executable provider-native tools allowed by the invocation context."""

    root = _workspace_root(context)
    enabled = set(context.executable_tools)
    tools: dict[str, BaseTool] = {}

    if "AskUser" in enabled:

        @tool("AskUser")
        def ask_user(question: str, required: bool = True) -> str:
            """Request bounded human input without blocking server execution."""

            status = "WAITING_FOR_USER" if required else "INPUT_REQUESTED"
            return _controlled_tool_result(
                "AskUser",
                f"Human input requested: {question[:1000]}",
                status=status,
            )

        tools["AskUser"] = ask_user

    if "WebSearch" in enabled:

        @tool("WebSearch")
        def web_search(query: str, max_results: int = MAX_SEARCH_RESULTS) -> str:
            """Search the web through configured Tavily or Playwright services."""

            return _web_search(query, max_results=max_results)

        tools["WebSearch"] = web_search

    if "Fetch" in enabled:

        @tool("Fetch")
        def fetch(url: str, timeout_seconds: int = FETCH_TIMEOUT_SECONDS) -> str:
            """Fetch a bounded text snippet from an http or https URL."""

            return _fetch_url(url, timeout_seconds=timeout_seconds)

        tools["Fetch"] = fetch

    if "AgentAsTool" in enabled:

        @tool("AgentAsTool")
        async def agent_as_tool(agent_id: str, task: str, instructions: str | None = None) -> str:
            """Delegate a bounded task to an explicitly bound assistant agent."""

            if agent_tool_executor is None:
                return _controlled_tool_result(
                    "AgentAsTool",
                    "Agent-as-tool delegation is not configured for this invocation.",
                )
            result = agent_tool_executor(agent_id, task, instructions)
            if hasattr(result, "__await__"):
                result = await result
            return str(result)

        tools["AgentAsTool"] = agent_as_tool

    if "RunSubAgent" in enabled:

        @tool("RunSubAgent")
        async def run_sub_agent(agent_id: str, task: str, instructions: str | None = None) -> str:
            """Delegate a bounded task through the sub-agent runtime when configured."""

            if agent_tool_executor is None:
                return _controlled_tool_result(
                    "RunSubAgent",
                    "Sub-agent delegation is not configured for this invocation.",
                )
            result = agent_tool_executor(agent_id, task, instructions)
            if hasattr(result, "__await__"):
                result = await result
            return str(result)

        tools["RunSubAgent"] = run_sub_agent

    if "GenerateImage" in enabled:

        @tool("GenerateImage")
        def generate_image(prompt: str, size: str | None = None) -> str:
            """Return a controlled setup-required result unless a media provider is configured."""

            _ = size
            return _controlled_tool_result(
                "GenerateImage",
                f"Image generation provider is not configured. Requested prompt: {prompt[:1000]}",
            )

        tools["GenerateImage"] = generate_image

    if "GenerateVideo" in enabled:

        @tool("GenerateVideo")
        def generate_video(prompt: str, duration_seconds: int | None = None) -> str:
            """Return a controlled setup-required result unless a video provider is configured."""

            _ = duration_seconds
            return _controlled_tool_result(
                "GenerateVideo",
                f"Video generation provider is not configured. Requested prompt: {prompt[:1000]}",
            )

        tools["GenerateVideo"] = generate_video

    if "SkillManager" in enabled:

        @tool("SkillManager")
        def skill_manager(action: str = "list", skill_name: str | None = None) -> str:
            """Inspect mounted skill metadata without arbitrary code loading."""

            skills = [
                {
                    "name": skill.name,
                    "description": skill.description,
                    "metadata": skill.metadata_ or {},
                }
                for skill in context.mounted_skills
            ]
            if action not in {"list", "inspect", "activate"}:
                return _controlled_tool_result(
                    "SkillManager", f"Unsupported skill action: {action}", status="FAILED"
                )
            if action in {"inspect", "activate"} and skill_name:
                matched_skills = [
                    skill
                    for skill in context.mounted_skills
                    if skill.name == skill_name
                ]
                matched = [
                    {
                        "name": skill.name,
                        "description": skill.description,
                        "metadata": skill.metadata_ or {},
                        "instructions": skill.body_markdown,
                    }
                    for skill in matched_skills
                ]
                return json.dumps(
                    {
                        "tool": "SkillManager",
                        "status": "COMPLETED" if matched else "NOT_FOUND",
                        "skills": matched,
                        "message": (
                            "Skill runtime activation records intent only; "
                            "no arbitrary code was loaded."
                        ),
                    },
                    ensure_ascii=False,
                )
            return json.dumps(
                {
                    "tool": "SkillManager",
                    "status": "COMPLETED",
                    "skills": skills,
                    "message": (
                        "Skill list includes metadata only; inspect or activate a skill "
                        "to load instructions."
                    ),
                },
                ensure_ascii=False,
            )

        tools["SkillManager"] = skill_manager

    if "TodoWrite" in enabled:

        @tool("TodoWrite")
        def todo_write(todos: list[str] | str) -> str:
            """Summarize transient task planning without persistent shared state."""

            todo_list = todos if isinstance(todos, list) else [todos]
            return json.dumps(
                {"tool": "TodoWrite", "status": "COMPLETED", "todos": todo_list[:20]},
                ensure_ascii=False,
            )

        tools["TodoWrite"] = todo_write

    if "ExitPlanMode" in enabled:

        @tool("ExitPlanMode")
        def exit_plan_mode(plan: str) -> str:
            """Return a controlled approval-needed planning result."""

            return _controlled_tool_result(
                "ExitPlanMode",
                f"Plan ready for user approval: {plan[:2000]}",
                status="APPROVAL_REQUIRED",
            )

        tools["ExitPlanMode"] = exit_plan_mode

    if root is None:
        if "Read" in enabled:

            @tool("Read")
            def read_unconfigured(
                file_path: str, start_line: int = 1, limit: int = MAX_READ_LINES
            ) -> str:
                """Return a workspace-required result when no local workspace is configured."""

                _ = (file_path, start_line, limit)
                return _controlled_tool_result(
                    "Read", "No local workspace is configured for this agent.", "WORKSPACE_REQUIRED"
                )

            tools["Read"] = read_unconfigured

        if "Write" in enabled:

            @tool("Write")
            def write_unconfigured(file_path: str, content: str) -> str:
                """Return a workspace-required result when no local workspace is configured."""

                _ = (file_path, content)
                return _controlled_tool_result(
                    "Write",
                    "No local workspace is configured for this agent.",
                    "WORKSPACE_REQUIRED",
                )

            tools["Write"] = write_unconfigured

        if "Edit" in enabled:

            @tool("Edit")
            def edit_unconfigured(
                file_path: str,
                old_string: str,
                new_string: str,
                replace_all: bool = False,
            ) -> str:
                """Return a workspace-required result when no local workspace is configured."""

                _ = (file_path, old_string, new_string, replace_all)
                return _controlled_tool_result(
                    "Edit", "No local workspace is configured for this agent.", "WORKSPACE_REQUIRED"
                )

            tools["Edit"] = edit_unconfigured

        if "Glob" in enabled:

            @tool("Glob")
            def glob_unconfigured(pattern: str = "**/*", limit: int = MAX_GLOB_RESULTS) -> str:
                """Return a workspace-required result when no local workspace is configured."""

                _ = (pattern, limit)
                return _controlled_tool_result(
                    "Glob", "No local workspace is configured for this agent.", "WORKSPACE_REQUIRED"
                )

            tools["Glob"] = glob_unconfigured

        if "Grep" in enabled:

            @tool("Grep")
            def grep_unconfigured(
                pattern: str, path: str = "**/*", limit: int = MAX_GREP_RESULTS
            ) -> str:
                """Return a workspace-required result when no local workspace is configured."""

                _ = (pattern, path, limit)
                return _controlled_tool_result(
                    "Grep", "No local workspace is configured for this agent.", "WORKSPACE_REQUIRED"
                )

            tools["Grep"] = grep_unconfigured

        if "Bash" in enabled:

            @tool("Bash")
            def bash_unconfigured(
                command: str, timeout_seconds: int = MAX_BASH_TIMEOUT_SECONDS
            ) -> str:
                """Return a workspace-required result when no local workspace is configured."""

                _ = (command, timeout_seconds)
                return _controlled_tool_result(
                    "Bash", "No local workspace is configured for this agent.", "WORKSPACE_REQUIRED"
                )

            tools["Bash"] = bash_unconfigured

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
    except NotImplementedError:
        return f"Tool {name!r} failed: async-only tool cannot execute in this sync path"
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
