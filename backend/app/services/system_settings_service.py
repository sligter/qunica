"""Per-user system settings (global preferences)."""

from dataclasses import dataclass
from pathlib import Path
from typing import Any
from urllib.parse import urlparse

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.exceptions import AgentChatError
from app.models.system_settings import SystemSettings
from app.models.user import User
from app.schemas.system_settings import SystemSettingsUpdate

DEFAULT_TAVILY_SEARCH_URL = "https://api.tavily.com/search"
DEFAULT_TAVILY_MAX_RESULTS = 5
DEFAULT_TAVILY_SEARCH_DEPTH = "basic"
TAVILY_SEARCH_DEPTHS = {"basic", "advanced"}
WEB_SEARCH_PROVIDERS = {"tavily"}


@dataclass(frozen=True, slots=True)
class TavilySearchConfig:
    api_key: str
    search_url: str
    max_results: int
    search_depth: str
    include_answer: bool
    include_raw_content: bool
    extra_params: dict[str, Any] | None = None


def _normalize_root(value: str | None) -> str | None:
    if value is None:
        return None
    stripped = value.strip()
    if not stripped:
        return None
    resolved = Path(stripped).expanduser().resolve()
    if not resolved.exists() or not resolved.is_dir():
        raise AgentChatError("group workspace root must be an existing directory")
    return str(resolved)


def _normalize_provider(value: str | None) -> str:
    provider = (value or "tavily").strip().lower()
    if provider not in WEB_SEARCH_PROVIDERS:
        raise AgentChatError("web search provider must be tavily")
    return provider


def _normalize_tavily_url(value: str | None) -> str:
    stripped = (value or "").strip()
    if not stripped:
        return DEFAULT_TAVILY_SEARCH_URL
    parsed = urlparse(stripped)
    if parsed.scheme not in {"http", "https"} or not parsed.netloc:
        raise AgentChatError("Tavily service URL must be an http or https URL")
    return stripped


def _normalize_tavily_key(value: str | None) -> str | None:
    if value is None:
        return None
    stripped = value.strip()
    return stripped or None


def _normalize_tavily_search_depth(value: str | None) -> str:
    depth = (value or DEFAULT_TAVILY_SEARCH_DEPTH).strip().lower()
    if depth not in TAVILY_SEARCH_DEPTHS:
        raise AgentChatError("Tavily search depth must be basic or advanced")
    return depth


def _normalize_tavily_max_results(value: int | None) -> int:
    max_results = value or DEFAULT_TAVILY_MAX_RESULTS
    if max_results < 1 or max_results > 20:
        raise AgentChatError("Tavily max results must be between 1 and 20")
    return max_results


async def get_or_create(db: AsyncSession, owner: User) -> SystemSettings:
    existing = await db.scalar(
        select(SystemSettings).where(SystemSettings.owner_id == owner.id)
    )
    if existing is not None:
        return existing
    settings = SystemSettings(owner_id=owner.id)
    db.add(settings)
    await db.flush()
    await db.refresh(settings)
    return settings


async def update(
    db: AsyncSession, owner: User, data: SystemSettingsUpdate
) -> SystemSettings:
    settings = await get_or_create(db, owner)
    if "group_workspace_root" in data.model_fields_set:
        settings.group_workspace_root = _normalize_root(data.group_workspace_root)
    if "web_search_provider" in data.model_fields_set:
        settings.web_search_provider = _normalize_provider(data.web_search_provider)
    if "tavily_api_key" in data.model_fields_set:
        settings.tavily_api_key = _normalize_tavily_key(data.tavily_api_key)
    if "tavily_search_url" in data.model_fields_set:
        settings.tavily_search_url = _normalize_tavily_url(data.tavily_search_url)
    if "tavily_max_results" in data.model_fields_set:
        settings.tavily_max_results = _normalize_tavily_max_results(data.tavily_max_results)
    if "tavily_search_depth" in data.model_fields_set:
        settings.tavily_search_depth = _normalize_tavily_search_depth(data.tavily_search_depth)
    if "tavily_include_answer" in data.model_fields_set:
        settings.tavily_include_answer = bool(data.tavily_include_answer)
    if "tavily_include_raw_content" in data.model_fields_set:
        settings.tavily_include_raw_content = bool(data.tavily_include_raw_content)
    await db.flush()
    await db.refresh(settings)
    return settings


async def require_group_workspace_root(db: AsyncSession, owner: User) -> str:
    settings = await get_or_create(db, owner)
    if not settings.group_workspace_root:
        raise AgentChatError(
            "group workspace root is not configured; set it in system settings"
        )
    return settings.group_workspace_root


def tavily_config_from_settings(settings: SystemSettings) -> TavilySearchConfig | None:
    if settings.web_search_provider != "tavily" or not settings.tavily_api_key:
        return None
    return TavilySearchConfig(
        api_key=settings.tavily_api_key,
        search_url=settings.tavily_search_url or DEFAULT_TAVILY_SEARCH_URL,
        max_results=settings.tavily_max_results,
        search_depth=settings.tavily_search_depth,
        include_answer=settings.tavily_include_answer,
        include_raw_content=settings.tavily_include_raw_content,
    )
