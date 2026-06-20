from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Literal, cast

from app.core.exceptions import AgentChatError

DEFAULT_TIMEOUT_SECONDS = 3600
MAX_TIMEOUT_SECONDS = 6 * 60 * 60
PermissionPolicy = Literal["deny", "auto_allow"]
AcpRuntimeProfile = Literal["custom", "codex", "claude"]
AcpConfigValue = str | bool

_LEGACY_ADAPTERS = {"codex", "claude_code"}
_RUNTIME_PROFILES = {"custom", "codex", "claude"}
_BLOCKED_ENV_KEYS = {
    "HOME",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "CODEX_HOME",
    "CLAUDE_CONFIG_DIR",
    "CLAUDE_HOME",
    "AG_SWARMER_EXTERNAL_AGENT",
    "AG_SWARMER_ACP_AGENT",
}


@dataclass(frozen=True, slots=True)
class AcpRuntimeConfig:
    command: str
    args: list[str]
    env: dict[str, str]
    timeout_seconds: int
    permission_policy: PermissionPolicy = "deny"
    profile: AcpRuntimeProfile = "custom"
    model: str | None = None
    mode: str | None = None
    thinking_effort: str | None = None
    config_options: dict[str, AcpConfigValue] | None = None


def normalize_acp_runtime(raw: dict[str, Any] | None) -> AcpRuntimeConfig:
    if not isinstance(raw, dict):
        raise AgentChatError("ACP runtime config is required for ACP agents")
    _reject_legacy_external_cli(raw)

    command = _normalize_required_text(raw.get("command"), "ACP runtime command")
    profile = _normalize_profile(raw.get("profile"))
    args = _normalize_args(raw.get("args"))
    env = _normalize_env(raw.get("env"))
    timeout_seconds = int(raw.get("timeout_seconds") or DEFAULT_TIMEOUT_SECONDS)
    if timeout_seconds < 1 or timeout_seconds > MAX_TIMEOUT_SECONDS:
        raise AgentChatError("ACP runtime timeout_seconds is out of range")
    permission_policy = raw.get("permission_policy") or "deny"
    if permission_policy not in {"deny", "auto_allow"}:
        raise AgentChatError("ACP runtime permission_policy must be deny or auto_allow")
    normalized_permission_policy = cast(PermissionPolicy, permission_policy)
    config_options = _normalize_config_options(raw.get("config_options"))

    return AcpRuntimeConfig(
        profile=profile,
        command=command,
        args=args,
        env=env,
        timeout_seconds=timeout_seconds,
        permission_policy=normalized_permission_policy,
        model=_normalize_optional_text(raw.get("model"), "ACP runtime model"),
        mode=_normalize_optional_text(raw.get("mode"), "ACP runtime mode"),
        thinking_effort=_normalize_optional_text(
            raw.get("thinking_effort"),
            "ACP runtime thinking_effort",
        ),
        config_options=config_options,
    )


def _reject_legacy_external_cli(raw: dict[str, Any]) -> None:
    adapter = raw.get("adapter")
    if adapter in _LEGACY_ADAPTERS:
        raise AgentChatError(
            "external CLI adapters are deprecated; configure this agent with an ACP "
            "runtime command instead"
        )
    if adapter is not None:
        raise AgentChatError("ACP runtime config must not include an adapter field")


def _normalize_required_text(value: Any, label: str) -> str:
    if not isinstance(value, str):
        raise AgentChatError(f"{label} is required")
    text = value.strip()
    if not text:
        raise AgentChatError(f"{label} is required")
    _reject_control_chars(text, label)
    return text


def _normalize_optional_text(value: Any, label: str) -> str | None:
    if value is None:
        return None
    if not isinstance(value, str):
        raise AgentChatError(f"{label} must be a string")
    text = value.strip()
    if not text:
        return None
    _reject_control_chars(text, label)
    return text


def _normalize_profile(value: Any) -> AcpRuntimeProfile:
    if value is None:
        return "custom"
    if not isinstance(value, str):
        raise AgentChatError("ACP runtime profile must be a string")
    profile = value.strip()
    if profile not in _RUNTIME_PROFILES:
        raise AgentChatError("ACP runtime profile must be custom, codex, or claude")
    return cast(AcpRuntimeProfile, profile)


def _normalize_args(value: Any) -> list[str]:
    if value is None:
        return []
    if not isinstance(value, list):
        raise AgentChatError("ACP runtime args must be a list of strings")
    args: list[str] = []
    for item in value:
        if not isinstance(item, str):
            raise AgentChatError("ACP runtime args must be a list of strings")
        _reject_nul(item, "ACP runtime arg")
        args.append(item)
    return args


def _normalize_env(value: Any) -> dict[str, str]:
    if value is None:
        return {}
    if not isinstance(value, dict):
        raise AgentChatError("ACP runtime env must be an object")
    env: dict[str, str] = {}
    for key, raw_value in value.items():
        if not isinstance(key, str) or not isinstance(raw_value, str):
            raise AgentChatError("ACP runtime env keys and values must be strings")
        if key in _BLOCKED_ENV_KEYS:
            raise AgentChatError(f"ACP runtime env may not override {key}")
        _reject_nul(key, "ACP runtime env key")
        _reject_nul(raw_value, "ACP runtime env value")
        env[key] = raw_value
    return env


def _normalize_config_options(value: Any) -> dict[str, AcpConfigValue] | None:
    if value is None:
        return None
    if not isinstance(value, dict):
        raise AgentChatError("ACP runtime config_options must be an object")
    config_options: dict[str, AcpConfigValue] = {}
    for key, raw_value in value.items():
        if not isinstance(key, str) or not key.strip():
            raise AgentChatError("ACP runtime config option keys must be strings")
        normalized_key = _normalize_required_text(key, "ACP runtime config option key")
        if isinstance(raw_value, bool):
            config_options[normalized_key] = raw_value
            continue
        if not isinstance(raw_value, str):
            raise AgentChatError("ACP runtime config option values must be strings or booleans")
        config_options[normalized_key] = _normalize_required_text(
            raw_value,
            "ACP runtime config option value",
        )
    return config_options or None


def _reject_control_chars(value: str, label: str) -> None:
    if any(char in value for char in ("\n", "\r", "\x00")):
        raise AgentChatError(f"{label} is invalid")


def _reject_nul(value: str, label: str) -> None:
    if "\x00" in value:
        raise AgentChatError(f"{label} is invalid")
