"""Provider-reported context usage extraction."""

from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any

from langchain_core.messages import AIMessage

from app.agents.context_budget import ContextBudget


@dataclass(frozen=True, slots=True)
class ContextUsage:
    input_tokens: int | None
    output_tokens: int | None
    total_tokens: int | None
    context_window_tokens: int
    output_reserve_tokens: int
    source: str

    @property
    def ratio(self) -> float | None:
        if self.input_tokens is None or self.context_window_tokens <= 0:
            return None
        return min(1.0, self.input_tokens / self.context_window_tokens)

    def to_payload(self) -> dict[str, int | float | str | None]:
        return {
            "input_tokens": self.input_tokens,
            "output_tokens": self.output_tokens,
            "total_tokens": self.total_tokens,
            "context_window_tokens": self.context_window_tokens,
            "output_reserve_tokens": self.output_reserve_tokens,
            "ratio": self.ratio,
            "source": self.source,
        }


def fallback_context_usage(
    *,
    input_tokens: int,
    context_budget: ContextBudget,
    source: str = "fallback_tokenizer",
) -> ContextUsage:
    return ContextUsage(
        input_tokens=input_tokens,
        output_tokens=None,
        total_tokens=None,
        context_window_tokens=context_budget.context_window_tokens,
        output_reserve_tokens=context_budget.output_reserve_tokens,
        source=source,
    )


def acp_context_usage(
    usage_update: Mapping[str, object],
    *,
    fallback_usage: ContextUsage,
) -> ContextUsage | None:
    used = _int_token_value(usage_update.get("used"))
    size = _int_token_value(usage_update.get("size"))
    if used is None or size is None or size <= 0:
        return None
    return ContextUsage(
        input_tokens=used,
        output_tokens=None,
        total_tokens=None,
        context_window_tokens=size,
        output_reserve_tokens=fallback_usage.output_reserve_tokens,
        source="provider",
    )


def extract_context_usage(
    response: AIMessage,
    *,
    fallback_usage: ContextUsage,
) -> ContextUsage:
    actual = _actual_usage_from_response(response)
    if actual is None:
        return fallback_usage
    input_tokens, output_tokens, total_tokens = actual
    return ContextUsage(
        input_tokens=input_tokens,
        output_tokens=output_tokens,
        total_tokens=total_tokens,
        context_window_tokens=fallback_usage.context_window_tokens,
        output_reserve_tokens=fallback_usage.output_reserve_tokens,
        source="provider",
    )


def _actual_usage_from_response(
    response: AIMessage,
) -> tuple[int | None, int | None, int | None] | None:
    usage_metadata = getattr(response, "usage_metadata", None)
    normalized = _usage_from_mapping(usage_metadata)
    if normalized is not None:
        return normalized

    response_metadata = getattr(response, "response_metadata", None)
    if isinstance(response_metadata, Mapping):
        token_usage = response_metadata.get("token_usage")
        normalized = _usage_from_mapping(token_usage)
        if normalized is not None:
            return normalized
        normalized = _usage_from_mapping(response_metadata)
        if normalized is not None:
            return normalized

    additional_kwargs = getattr(response, "additional_kwargs", None)
    if isinstance(additional_kwargs, Mapping):
        normalized = _usage_from_mapping(additional_kwargs.get("usage"))
        if normalized is not None:
            return normalized
    return None


def _usage_from_mapping(value: object) -> tuple[int | None, int | None, int | None] | None:
    if not isinstance(value, Mapping):
        return None
    input_tokens = _int_from_keys(value, ("input_tokens", "prompt_tokens"))
    output_tokens = _int_from_keys(value, ("output_tokens", "completion_tokens"))
    total_tokens = _int_from_keys(value, ("total_tokens",))
    if input_tokens is None and output_tokens is None and total_tokens is None:
        return None
    if total_tokens is None and input_tokens is not None and output_tokens is not None:
        total_tokens = input_tokens + output_tokens
    return input_tokens, output_tokens, total_tokens


def _int_from_keys(value: Mapping[Any, Any], keys: tuple[str, ...]) -> int | None:
    for key in keys:
        normalized = _int_token_value(value.get(key))
        if normalized is not None:
            return normalized
    return None


def _int_token_value(raw: object) -> int | None:
    if isinstance(raw, bool):
        return None
    if isinstance(raw, int):
        return raw
    if isinstance(raw, float):
        return int(raw)
    return None
