"""LangChain ChatModel factory.

Two construction paths:

1. `make_chat_model(llm_config)` — legacy path used by tests' fake_llm
   monkey-patch. Builds a `ChatOpenAI` from `app.core.config.settings`
   (with optional per-call overrides).
2. `resolve_chat_model(db, agent, streaming)` — Phase A path that picks the
   right ChatModel based on the agent's `llm_provider_id`. Falls back to
   `make_chat_model` when the agent has no provider attached.

Supported provider kinds:
- 'openai-compatible' — ChatOpenAI with a custom base_url. Covers OpenAI,
  DeepSeek, Qwen, MiMo, Together, OpenRouter.
- 'anthropic' — ChatAnthropic for Claude, supports custom base_url.
- 'gemini' — ChatGoogleGenerativeAI for Google Gemini models.
"""

from collections.abc import Mapping
from typing import Any

from langchain_anthropic import ChatAnthropic
from langchain_core.language_models import BaseChatModel
from langchain_core.outputs import ChatGenerationChunk, ChatResult
from langchain_google_genai import ChatGoogleGenerativeAI
from langchain_openai import ChatOpenAI
from pydantic import SecretStr
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.config import settings
from app.core.exceptions import LLMProviderError
from app.models.agent import Agent
from app.services import llm_provider_service

_REASONING_EFFORTS: frozenset[str] = frozenset({"low", "medium", "high", "xhigh"})

# OpenAI-compatible reasoning models (DeepSeek-R1, Qwen, GLM, MiniMax, …) return
# the chain of thought in a non-standard `reasoning_content` (a few use
# `reasoning`) field. `langchain-openai` >= 1.x deliberately drops these because
# `ChatOpenAI` targets the official OpenAI spec only, so the agent runtime never
# sees the thinking stream. `ReasoningChatOpenAI` re-injects it into
# `additional_kwargs["reasoning_content"]`, which `runtime` already knows how to
# surface as a "reasoning" stream part.
_REASONING_CONTENT_KEYS = ("reasoning_content", "reasoning")


def _reasoning_text_from_payload(payload: Any) -> str | None:
    if not isinstance(payload, Mapping):
        return None
    for key in _REASONING_CONTENT_KEYS:
        value = payload.get(key)
        if isinstance(value, str) and value:
            return value
    return None


class ReasoningChatOpenAI(ChatOpenAI):
    """`ChatOpenAI` that preserves `reasoning_content` from OpenAI-compatible providers.

    Upstream strips provider-specific fields; we restore the thinking stream on
    both the streaming and non-streaming paths so it can be rendered in the UI.
    Each streamed delta carries only its own slice of reasoning, which LangChain
    concatenates across chunks, so the aggregated final message also ends up with
    the full `reasoning_content`.
    """

    def _convert_chunk_to_generation_chunk(
        self,
        chunk: dict,
        default_chunk_class: type,
        base_generation_info: dict | None,
    ) -> ChatGenerationChunk | None:
        generation_chunk = super()._convert_chunk_to_generation_chunk(
            chunk, default_chunk_class, base_generation_info
        )
        if generation_chunk is None:
            return generation_chunk
        choices = chunk.get("choices") or chunk.get("chunk", {}).get("choices", [])
        if choices:
            delta = choices[0].get("delta") or {}
            reasoning = _reasoning_text_from_payload(delta)
            if reasoning:
                generation_chunk.message.additional_kwargs["reasoning_content"] = reasoning
        return generation_chunk

    def _create_chat_result(
        self,
        response: Any,
        generation_info: dict | None = None,
    ) -> ChatResult:
        result = super()._create_chat_result(response, generation_info)
        response_dict = response if isinstance(response, dict) else response.model_dump()
        choices = response_dict.get("choices") or []
        for generation, choice in zip(result.generations, choices, strict=False):
            reasoning = _reasoning_text_from_payload(choice.get("message"))
            if reasoning:
                generation.message.additional_kwargs.setdefault("reasoning_content", reasoning)
        return result


def _extract_common_params(cfg: dict[str, Any]) -> dict[str, Any]:
    """Extract common model parameters from llm_config."""
    params: dict[str, Any] = {}
    if "temperature" in cfg:
        params["temperature"] = float(cfg["temperature"])
    if "top_p" in cfg:
        params["top_p"] = float(cfg["top_p"])
    if "max_tokens" in cfg:
        params["max_tokens"] = int(cfg["max_tokens"])
    return params


def _extract_reasoning_effort(cfg: dict[str, Any]) -> str | None:
    value = cfg.get("reasoning_effort")
    if isinstance(value, str) and value in _REASONING_EFFORTS:
        return value
    return None


def _provider_reasoning_params(provider_kind: str, cfg: dict[str, Any]) -> dict[str, Any]:
    reasoning_effort = _extract_reasoning_effort(cfg)
    if reasoning_effort is None:
        return {}
    if provider_kind in {"anthropic", "anthropic-compatible"}:
        return {"effort": reasoning_effort}
    if provider_kind == "gemini":
        return {"thinking_level": reasoning_effort}
    return {"reasoning_effort": reasoning_effort}


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

    kwargs: dict[str, Any] = {
        "model": model,
        "api_key": api_key,
        "base_url": base_url,
        "temperature": temperature,
    }
    if "top_p" in cfg:
        kwargs["top_p"] = float(cfg["top_p"])
    if "max_tokens" in cfg:
        kwargs["max_tokens"] = int(cfg["max_tokens"])
    kwargs.update(_provider_reasoning_params("openai-compatible", cfg))

    return ReasoningChatOpenAI(**kwargs)


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
        cm = make_chat_model(overrides)
        if streaming and hasattr(cm, "streaming"):
            cm.streaming = True
        return cm

    provider = await llm_provider_service.get_for_use(db, agent.llm_provider_id)
    model_name = overrides.get("model") or provider.default_model
    temperature = float(overrides.get("temperature", 0.7))
    extra = _extract_common_params(overrides)
    extra.pop("temperature", None)  # handled explicitly
    extra.update(_provider_reasoning_params(provider.kind, overrides))

    if provider.kind in {"anthropic", "anthropic-compatible"}:
        kwargs: dict[str, Any] = {
            "model": model_name,
            "api_key": provider.api_key,
            "temperature": temperature,
            "streaming": streaming,
            **extra,
        }
        if provider.base_url:
            kwargs["anthropic_api_url"] = provider.base_url
        return ChatAnthropic(**kwargs)

    if provider.kind == "gemini":
        kwargs = {
            "model": model_name,
            "google_api_key": provider.api_key,
            "temperature": temperature,
            "streaming": streaming,
            **extra,
        }
        return ChatGoogleGenerativeAI(**kwargs)

    # 'openai-compatible' covers everything else
    base_url = provider.base_url or settings.llm_base_url
    return ReasoningChatOpenAI(
        model=model_name,
        api_key=SecretStr(provider.api_key),
        base_url=base_url,
        temperature=temperature,
        streaming=streaming,
        **extra,
    )
