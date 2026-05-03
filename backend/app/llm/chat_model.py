"""LangChain ChatModel factory.

Two construction paths:

1. `make_chat_model(llm_config)` — legacy path used by tests' fake_llm
   monkey-patch. Builds a `ChatOpenAI` from `app.core.config.settings`
   (with optional per-call overrides).
2. `resolve_chat_model(db, agent, streaming)` — Phase A path that picks the
   right ChatModel based on the agent's `llm_provider_id`. Falls back to
   `make_chat_model` when the agent has no provider attached.

Supported provider kinds (V1):
- 'openai-compatible' — ChatOpenAI with a custom base_url. Covers OpenAI,
  DeepSeek, Qwen, MiMo, Together, OpenRouter, Gemini's OpenAI-compat mode.
- 'anthropic' — ChatAnthropic for Claude.

Other vendors route through 'openai-compatible' by setting their custom
base_url (e.g. https://generativelanguage.googleapis.com/v1beta/openai for
Gemini).
"""

from typing import Any

from langchain_anthropic import ChatAnthropic
from langchain_core.language_models import BaseChatModel
from langchain_openai import ChatOpenAI
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.config import settings
from app.core.exceptions import LLMProviderError
from app.models.agent import Agent
from app.services import llm_provider_service


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


async def resolve_chat_model(
    db: AsyncSession, agent: Agent, *, streaming: bool = False
) -> BaseChatModel:
    """Pick the right ChatModel based on the agent's provider.

    Falls back to `make_chat_model` (env-default OpenAI-compat) when the
    agent has no `llm_provider_id`. Per-agent `llm_config` overrides only
    the model name; api_key + base_url come from the provider record.
    """
    overrides = agent.llm_config or {}
    if agent.llm_provider_id is None:
        # Env default; honour streaming via wrapper since make_chat_model
        # doesn't accept streaming param yet.
        cm = make_chat_model(overrides)
        if streaming and hasattr(cm, "streaming"):
            cm.streaming = True
        return cm

    provider = await llm_provider_service.get_for_use(db, agent.llm_provider_id)
    model_name = overrides.get("model") or provider.default_model
    temperature = float(overrides.get("temperature", 0.7))

    if provider.kind == "anthropic":
        return ChatAnthropic(
            model=model_name,
            api_key=provider.api_key,
            temperature=temperature,
            streaming=streaming,
        )
    # 'openai-compatible' covers everything else
    base_url = provider.base_url or settings.llm_base_url
    return ChatOpenAI(
        model=model_name,
        api_key=provider.api_key,
        base_url=base_url,
        temperature=temperature,
        streaming=streaming,
    )
