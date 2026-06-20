from __future__ import annotations

import json
import os
import shutil
import sys
import tomllib
from collections.abc import Iterable
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Literal, cast

from app.external_agents.adapters import AcpRuntimeProfile, PermissionPolicy

# JSON/TOML parsers return untyped data; every helper below validates shape before use.
JsonObject = dict[str, Any]


@dataclass(frozen=True, slots=True)
class AcpRuntimeChoice:
    value: str
    label: str
    description: str | None = None


@dataclass(frozen=True, slots=True)
class AcpRuntimePreset:
    id: Literal["codex", "claude"]
    name: str
    description: str
    profile: AcpRuntimeProfile
    installed: bool
    command: str | None
    args: list[str] = field(default_factory=list)
    env: dict[str, str] = field(default_factory=dict)
    timeout_seconds: int = 3600
    permission_policy: PermissionPolicy = "deny"
    default_model: str | None = None
    default_mode: str | None = None
    default_thinking_effort: str | None = None
    model_options: list[AcpRuntimeChoice] = field(default_factory=list)
    mode_options: list[AcpRuntimeChoice] = field(default_factory=list)
    thinking_effort_options: list[AcpRuntimeChoice] = field(default_factory=list)
    install_hint: str = ""
    source: str | None = None


@dataclass(frozen=True, slots=True)
class LocalModelDiscovery:
    default_model: str | None
    options: list[AcpRuntimeChoice]


def discover_acp_runtime_presets() -> list[AcpRuntimePreset]:
    codex_command = _find_command("codex-acp")
    claude_command = _find_command("claude-agent-acp")
    npx_command = _find_command("npx")
    return [
        _codex_preset(codex_command, npx_command),
        _claude_preset(claude_command, npx_command),
    ]


def _codex_preset(command: str | None, npx_command: str | None) -> AcpRuntimePreset:
    runtime_command = command or npx_command or "codex-acp"
    models = _discover_codex_models()
    return AcpRuntimePreset(
        id="codex",
        name="Codex",
        description="Codex CLI through the Zed Codex ACP adapter.",
        profile="codex",
        installed=command is not None,
        command=runtime_command,
        args=[] if command else (["@zed-industries/codex-acp"] if npx_command else []),
        default_mode="read-only",
        default_thinking_effort="medium",
        default_model=models.default_model,
        model_options=models.options,
        mode_options=[
            AcpRuntimeChoice(
                "read-only",
                "Read Only",
                "Read files in the current workspace; ask before edits or internet.",
            ),
            AcpRuntimeChoice(
                "auto",
                "Default",
                "Read and edit workspace files; ask for internet or external edits.",
            ),
            AcpRuntimeChoice(
                "full-access",
                "Full Access",
                "Edit outside the workspace and access the internet without asking.",
            ),
        ],
        thinking_effort_options=[
            AcpRuntimeChoice("", "Default"),
            AcpRuntimeChoice("minimal", "Minimal"),
            AcpRuntimeChoice("low", "Low"),
            AcpRuntimeChoice("medium", "Medium"),
            AcpRuntimeChoice("high", "High"),
            AcpRuntimeChoice("xhigh", "XHigh"),
        ],
        install_hint=(
            "Install @zed-industries/codex-acp so codex-acp is on PATH, "
            "or keep the npx fallback command."
        ),
        source="PATH" if command else ("npx" if npx_command else None),
    )


def _claude_preset(command: str | None, npx_command: str | None) -> AcpRuntimePreset:
    runtime_command = command or npx_command or "claude-agent-acp"
    models = _discover_claude_models()
    return AcpRuntimePreset(
        id="claude",
        name="Claude Code",
        description="Claude Agent SDK through the official Claude Agent ACP adapter.",
        profile="claude",
        installed=command is not None,
        command=runtime_command,
        args=[] if command else (["@agentclientprotocol/claude-agent-acp"] if npx_command else []),
        default_model=models.default_model,
        default_mode="default",
        default_thinking_effort="high",
        model_options=models.options,
        mode_options=[
            AcpRuntimeChoice("default", "Default"),
            AcpRuntimeChoice(
                "auto",
                "Auto",
                "Use a model classifier to approve or deny permission prompts.",
            ),
            AcpRuntimeChoice("acceptEdits", "Accept Edits"),
            AcpRuntimeChoice("plan", "Plan"),
            AcpRuntimeChoice("dontAsk", "Don't Ask"),
            AcpRuntimeChoice("bypassPermissions", "Bypass Permissions"),
        ],
        thinking_effort_options=[
            AcpRuntimeChoice("low", "Low"),
            AcpRuntimeChoice("medium", "Medium"),
            AcpRuntimeChoice("high", "High"),
            AcpRuntimeChoice("max", "Max"),
        ],
        install_hint=(
            "Install @agentclientprotocol/claude-agent-acp and ensure "
            "claude-agent-acp is on PATH, or keep the npx fallback command."
        ),
        source="PATH" if command else ("npx" if npx_command else None),
    )


def _find_command(command: str) -> str | None:
    path_command = shutil.which(command)
    if path_command:
        return path_command
    for path in _local_bin_candidates(command):
        if path.is_file():
            return str(path)
    return None


def _discover_codex_models() -> LocalModelDiscovery:
    configured_models: list[str] = []
    default_model: str | None = None
    for config_file in _codex_config_files():
        config = _read_toml_object(config_file)
        if not config:
            continue
        model = _as_non_empty_string(config.get("model"))
        if model is not None:
            default_model = model
            configured_models.append(model)
        configured_models.extend(_extract_codex_profile_models(config))

    cache_options = _codex_cached_model_options()
    cache_by_value = {option.value: option for option in cache_options}
    ordered_options: list[AcpRuntimeChoice] = []
    if default_model is not None:
        ordered_options.append(
            cache_by_value.get(
                default_model,
                AcpRuntimeChoice(
                    default_model,
                    _format_model_label(default_model),
                    "Configured as the Codex default model.",
                ),
            )
        )
    ordered_options.extend(cache_options)
    for model in configured_models:
        ordered_options.append(
            cache_by_value.get(
                model,
                AcpRuntimeChoice(
                    model,
                    _format_model_label(model),
                    "Configured in Codex config.toml.",
                ),
            )
        )
    return LocalModelDiscovery(
        default_model=default_model,
        options=_dedupe_choices(ordered_options),
    )


def _codex_config_files() -> list[Path]:
    return _existing_files(root / "config.toml" for root in _codex_home_candidates())


def _codex_cached_model_options() -> list[AcpRuntimeChoice]:
    options: list[AcpRuntimeChoice] = []
    cache_files = _existing_files(
        root / "models_cache.json" for root in _codex_home_candidates()
    )
    for cache_file in cache_files:
        raw_models = _read_json_object(cache_file).get("models")
        if not isinstance(raw_models, list):
            continue
        for raw_model in raw_models:
            if not isinstance(raw_model, dict):
                continue
            model = cast(JsonObject, raw_model)
            value = _as_non_empty_string(
                model.get("slug") or model.get("id") or model.get("model")
            )
            if value is None:
                continue
            visibility = _as_non_empty_string(model.get("visibility"))
            if visibility == "hide":
                continue
            label = _as_non_empty_string(
                model.get("display_name") or model.get("name") or model.get("label")
            )
            description = _as_non_empty_string(model.get("description"))
            options.append(
                AcpRuntimeChoice(
                    value,
                    label or _format_model_label(value),
                    description,
                )
            )
    return _dedupe_choices(options)


def _codex_home_candidates() -> list[Path]:
    userprofile = _env_path("USERPROFILE") or Path.home()
    candidates = [
        _env_path("CODEX_HOME"),
        userprofile / ".codex",
        Path.home() / ".codex",
    ]
    return _dedupe_paths(path for path in candidates if path is not None)


def _extract_codex_profile_models(config: JsonObject) -> list[str]:
    profiles = config.get("profiles")
    if not isinstance(profiles, dict):
        return []
    models: list[str] = []
    for raw_profile in profiles.values():
        if not isinstance(raw_profile, dict):
            continue
        model = _as_non_empty_string(raw_profile.get("model"))
        if model is not None:
            models.append(model)
    return models


def _discover_claude_models() -> LocalModelDiscovery:
    default_model: str | None = None
    options: list[AcpRuntimeChoice] = []
    for settings_file in _claude_settings_files():
        settings = _read_json_object(settings_file)
        if not settings:
            continue
        model = _as_non_empty_string(settings.get("model"))
        if model is not None:
            default_model = model
            options.append(
                AcpRuntimeChoice(
                    model,
                    _format_claude_model_label(model),
                    "Configured as the Claude Code default model.",
                )
            )
        options.extend(_extract_claude_env_model_options(settings.get("env")))
        options.extend(_extract_claude_available_model_options(settings.get("availableModels")))
        options.extend(_extract_claude_model_config_options(settings.get("modelConfig")))

    return LocalModelDiscovery(
        default_model=default_model,
        options=_dedupe_choices(options),
    )


def _claude_settings_files() -> list[Path]:
    config_dirs = _claude_config_dir_candidates()
    candidates = [config_dir / "settings.json" for config_dir in config_dirs]
    candidates.extend(config_dir / "settings.local.json" for config_dir in config_dirs)
    cwd = Path.cwd()
    candidates.extend(
        [
            cwd / ".claude" / "settings.json",
            cwd / ".claude" / "settings.local.json",
            cwd.parent / ".claude" / "settings.json",
            cwd.parent / ".claude" / "settings.local.json",
            _managed_claude_settings_path(),
            (_env_path("USERPROFILE") or Path.home()) / ".claude.json",
        ]
    )
    appdata = _env_path("APPDATA")
    if appdata is not None:
        candidates.append(appdata / "Claude" / "settings.json")
    return _existing_files(path for path in candidates if path is not None)


def _claude_config_dir_candidates() -> list[Path]:
    userprofile = _env_path("USERPROFILE") or Path.home()
    candidates = [
        _env_path("CLAUDE_CONFIG_DIR"),
        _env_path("CLAUDE_HOME"),
        userprofile / ".claude",
        Path.home() / ".claude",
    ]
    return _dedupe_paths(path for path in candidates if path is not None)


def _managed_claude_settings_path() -> Path:
    if os.name == "nt":
        program_files = _env_path("ProgramFiles") or Path("C:/Program Files")
        return program_files / "ClaudeCode" / "managed-settings.json"
    if sys.platform == "darwin":
        return Path("/Library/Application Support/ClaudeCode/managed-settings.json")
    return Path("/etc/claude-code/managed-settings.json")


def _extract_claude_env_model_options(raw_env: object) -> list[AcpRuntimeChoice]:
    if not isinstance(raw_env, dict):
        return []
    env = cast(JsonObject, raw_env)
    options: list[AcpRuntimeChoice] = []
    for key, value in env.items():
        if key == "CLAUDE_MODEL_CONFIG":
            options.extend(_extract_claude_model_config_options(value))
            continue
        if "MODEL" not in key:
            continue
        model = _as_non_empty_string(value)
        if model is None:
            continue
        options.append(
            AcpRuntimeChoice(
                model,
                _format_claude_model_label(model),
                f"Configured by Claude Code env {key}.",
            )
        )
    return options


def _extract_claude_model_config_options(raw_config: object) -> list[AcpRuntimeChoice]:
    model_config: object = raw_config
    if isinstance(raw_config, str):
        try:
            model_config = json.loads(raw_config)
        except json.JSONDecodeError:
            return []
    if not isinstance(model_config, dict):
        return []
    config = cast(JsonObject, model_config)
    options = _extract_claude_available_model_options(config.get("availableModels"))
    model_overrides = config.get("modelOverrides")
    if isinstance(model_overrides, dict):
        for key, value in model_overrides.items():
            model = _as_non_empty_string(value) or _as_non_empty_string(key)
            if model is None:
                continue
            options.append(
                AcpRuntimeChoice(
                    model,
                    _format_claude_model_label(model),
                    "Configured in Claude model overrides.",
                )
            )
    return options


def _extract_claude_available_model_options(raw_models: object) -> list[AcpRuntimeChoice]:
    if not isinstance(raw_models, list):
        return []
    options: list[AcpRuntimeChoice] = []
    for raw_model in raw_models:
        if isinstance(raw_model, str):
            options.append(
                AcpRuntimeChoice(
                    raw_model,
                    _format_claude_model_label(raw_model),
                    "Configured in Claude availableModels.",
                )
            )
            continue
        if not isinstance(raw_model, dict):
            continue
        model = cast(JsonObject, raw_model)
        value = _as_non_empty_string(
            model.get("modelId")
            or model.get("model_id")
            or model.get("id")
            or model.get("value")
            or model.get("model")
        )
        if value is None:
            continue
        label = _as_non_empty_string(
            model.get("name") or model.get("displayName") or model.get("display_name")
        )
        description = _as_non_empty_string(model.get("description"))
        options.append(
            AcpRuntimeChoice(
                value,
                label or _format_claude_model_label(value),
                description or "Configured in Claude availableModels.",
            )
        )
    return options


def _read_json_object(path: Path) -> JsonObject:
    try:
        with path.open(encoding="utf-8") as file:
            parsed = json.load(file)
    except (OSError, json.JSONDecodeError, UnicodeDecodeError):
        return {}
    return cast(JsonObject, parsed) if isinstance(parsed, dict) else {}


def _read_toml_object(path: Path) -> JsonObject:
    try:
        with path.open("rb") as file:
            parsed = tomllib.load(file)
    except (OSError, tomllib.TOMLDecodeError):
        return {}
    return parsed


def _as_non_empty_string(value: object) -> str | None:
    if not isinstance(value, str):
        return None
    text = value.strip()
    return text or None


def _format_model_label(value: str) -> str:
    return value.replace("-", " ").title().replace("Gpt", "GPT")


def _format_claude_model_label(value: str) -> str:
    normalized = value.strip()
    lower = normalized.lower()
    if lower == "opus[1m]":
        return "Opus (1M context)"
    if lower == "sonnet[1m]":
        return "Sonnet (1M context)"
    if lower == "haiku":
        return "Haiku"
    return normalized


def _dedupe_choices(options: list[AcpRuntimeChoice]) -> list[AcpRuntimeChoice]:
    choices: dict[str, AcpRuntimeChoice] = {}
    for option in options:
        if option.value and option.value not in choices:
            choices[option.value] = option
    return list(choices.values())


def _existing_files(paths: Iterable[Path]) -> list[Path]:
    return [path for path in _dedupe_paths(paths) if path.is_file()]


def _dedupe_paths(paths: Iterable[Path]) -> list[Path]:
    unique: dict[Path, None] = {}
    for path in paths:
        unique[path] = None
    return list(unique)


def _local_bin_candidates(command: str) -> list[Path]:
    extensions = [""] if os.name != "nt" else [".cmd", ".exe", ".bat", ".ps1", ""]
    roots = _candidate_roots()
    return [root / f"{command}{extension}" for root in roots for extension in extensions]


def _candidate_roots() -> list[Path]:
    cwd = Path.cwd()
    candidates = [
        cwd / "node_modules" / ".bin",
        cwd.parent / "node_modules" / ".bin",
        cwd / "frontend" / "node_modules" / ".bin",
        cwd.parent / "frontend" / "node_modules" / ".bin",
    ]
    candidates.extend(_known_user_bin_roots())
    candidates.extend(_npx_cache_bin_roots())
    return list(dict.fromkeys(candidates))


def _known_user_bin_roots() -> list[Path]:
    roots: list[Path] = []
    appdata = _env_path("APPDATA")
    local_appdata = _env_path("LOCALAPPDATA")
    userprofile = _env_path("USERPROFILE") or Path.home()
    program_files = _env_path("ProgramFiles")
    program_files_x86 = _env_path("ProgramFiles(x86)")

    if appdata is not None:
        roots.append(appdata / "npm")
    if local_appdata is not None:
        roots.extend(
            [
                local_appdata / "pnpm",
                local_appdata / "Volta" / "bin",
                local_appdata / "Programs" / "nodejs",
            ]
        )
    if userprofile is not None:
        roots.extend(
            [
                userprofile / ".local" / "bin",
                userprofile / "scoop" / "shims",
            ]
        )
    if program_files is not None:
        roots.append(program_files / "nodejs")
    if program_files_x86 is not None:
        roots.append(program_files_x86 / "nodejs")
    return roots


def _npx_cache_bin_roots() -> list[Path]:
    roots: list[Path] = []
    cache_roots = [
        _env_path("npm_config_cache"),
        _env_path("NPM_CONFIG_CACHE"),
    ]
    local_appdata = _env_path("LOCALAPPDATA")
    if local_appdata is not None:
        cache_roots.append(local_appdata / "npm-cache")

    for cache_root in cache_roots:
        npx_root = cache_root / "_npx" if cache_root is not None else None
        if npx_root is None or not npx_root.is_dir():
            continue
        roots.extend(path / "node_modules" / ".bin" for path in npx_root.iterdir() if path.is_dir())
    return roots


def _env_path(name: str) -> Path | None:
    value = os.environ.get(name)
    return Path(value) if value else None
