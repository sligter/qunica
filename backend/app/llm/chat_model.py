"""LangChain ChatModel factory.

Single LLM client used across the whole codebase: the LangGraph agent_node
and the stateless `/agents/{id}/invoke[/stream]` endpoints both go through
`make_chat_model`. Per-agent `llm_config` (a dict on the Agent row) overrides
the corresponding fields from `app.core.config.settings`.
"""

from typing import Any

from langchain_core.language_models import BaseChatModel
from langchain_openai import ChatOpenAI

from app.core.config import settings
from app.core.exceptions import LLMProviderError


def make_chat_model(
    llm_config: dict[str, Any] | None = None,
) -> BaseChatModel:
    cfg = llm_config or {}
    model = cfg.get("model") or settings.llm_default_model
    api_key = cfg.get("api_key") or settings.llm_api_key
    base_url = cfg.get("base_url") or settings.llm_base_url
    temperature = float(cfg.get("temperature", 0.7))

    if not api_key:
        raise LLMProviderError("no LLM api_key configured")

    return ChatOpenAI(
        model=model,
        api_key=api_key,
        base_url=base_url,
        temperature=temperature,
    )
