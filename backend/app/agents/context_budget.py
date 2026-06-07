"""Token-budgeted chat history assembly for agent invocations."""

from __future__ import annotations

import math
from collections.abc import Mapping
from dataclasses import dataclass
from datetime import datetime
from functools import lru_cache
from importlib import import_module
from types import ModuleType
from uuid import UUID

from langchain_core.messages import AIMessage, BaseMessage, HumanMessage, SystemMessage

from app.models.agent import Agent

DEFAULT_CONTEXT_WINDOW_TOKENS = 32_000
OUTPUT_RESERVE_RATIO = 0.30
MESSAGE_TOKEN_OVERHEAD = 4
SUMMARY_TOKEN_CAP = 1_200
SUMMARY_INPUT_FRACTION_CAP = 0.15
MIN_REQUIRED_MESSAGE_TOKENS = 256
MIN_OPTIONAL_TRUNCATED_MESSAGE_TOKENS = 128
ROLLING_SUMMARY_TOKEN_CAP = 2_400
SUMMARY_ITEM_TOKEN_CAP = 160

CONTEXT_WINDOW_CONFIG_KEYS = (
    "context_window_tokens",
    "context_window",
    "max_context_tokens",
    "model_context_window",
)
OUTPUT_RESERVE_RATIO_CONFIG_KEYS = (
    "context_output_reserve_ratio",
    "output_reserve_ratio",
)

MODEL_CONTEXT_WINDOWS: tuple[tuple[str, int], ...] = (
    ("gpt-4.1", 1_000_000),
    ("gpt-4o", 128_000),
    ("gpt-5", 400_000),
    ("o3", 200_000),
    ("o4", 200_000),
    ("claude-3.5", 200_000),
    ("claude-3-5", 200_000),
    ("claude-3.7", 200_000),
    ("claude-3-7", 200_000),
    ("claude-4", 200_000),
    ("gemini-1.5", 1_000_000),
    ("gemini-2", 1_000_000),
    ("deepseek", 64_000),
    ("qwen", 128_000),
)


@dataclass(frozen=True, slots=True)
class ContextBudget:
    context_window_tokens: int
    output_reserve_tokens: int
    input_budget_tokens: int


@dataclass(frozen=True, slots=True)
class ContextHistoryItem:
    message_id: UUID
    created_at: datetime
    sender_label: str
    message: BaseMessage
    raw_content: str


@dataclass(frozen=True, slots=True)
class BudgetedContext:
    messages: list[BaseMessage]
    dropped_items: list[ContextHistoryItem]
    context_budget: ContextBudget
    fallback_input_tokens: int


def resolve_context_budget(
    agent: Agent,
    *,
    model_name: str | None = None,
    provider_context_window_tokens: int | None = None,
    provider_output_reserve_ratio: float | None = None,
) -> ContextBudget:
    """Resolve the model context window and reserve 30% for output."""
    config = _agent_config(agent)
    context_window = _configured_context_window(config)
    if context_window is None:
        context_window = provider_context_window_tokens
    if context_window is None:
        resolved_model = model_name or _configured_model_name(config)
        context_window = _model_default_context_window(resolved_model)

    reserve_ratio = _configured_output_reserve_ratio(config)
    if reserve_ratio is None:
        reserve_ratio = provider_output_reserve_ratio
    if reserve_ratio is None:
        reserve_ratio = OUTPUT_RESERVE_RATIO

    output_reserve = max(1, math.floor(context_window * reserve_ratio))
    input_budget = max(1, context_window - output_reserve)
    return ContextBudget(
        context_window_tokens=context_window,
        output_reserve_tokens=output_reserve,
        input_budget_tokens=input_budget,
    )


def build_budgeted_context(
    *,
    system_message: SystemMessage,
    history_items: list[ContextHistoryItem],
    rolling_summary: str | None,
    context_budget: ContextBudget,
    required_message_id: UUID | None = None,
    extra_user_text: str | None = None,
) -> BudgetedContext:
    """Build input messages by preserving required turns and backfilling recent history."""
    selected_by_id: dict[UUID, BaseMessage] = {}
    budget = context_budget.input_budget_tokens
    used_tokens = estimate_message_tokens(system_message)

    summary_message = _summary_message(rolling_summary, budget - used_tokens)
    if summary_message is not None:
        used_tokens += estimate_message_tokens(summary_message)

    required_ids: set[UUID] = set()
    if required_message_id is not None:
        required_ids.add(required_message_id)
    elif history_items:
        required_ids.add(history_items[-1].message_id)

    for item in history_items:
        if item.message_id not in required_ids:
            continue
        available = max(budget - used_tokens, MIN_REQUIRED_MESSAGE_TOKENS)
        selected = fit_message_to_token_budget(item.message, available)
        selected_by_id[item.message_id] = selected
        used_tokens += estimate_message_tokens(selected)

    extra_message: HumanMessage | None = None
    if extra_user_text:
        available = max(budget - used_tokens, MIN_REQUIRED_MESSAGE_TOKENS)
        extra_message = HumanMessage(
            content=truncate_text_to_token_budget(extra_user_text, available)
        )
        used_tokens += estimate_message_tokens(extra_message)

    for item in reversed(history_items):
        if item.message_id in selected_by_id:
            continue
        available = budget - used_tokens
        if available <= 0:
            continue

        item_tokens = estimate_message_tokens(item.message)
        if item_tokens <= available:
            selected_by_id[item.message_id] = item.message
            used_tokens += item_tokens
            continue

        if available >= MIN_OPTIONAL_TRUNCATED_MESSAGE_TOKENS:
            selected = fit_message_to_token_budget(item.message, available)
            selected_by_id[item.message_id] = selected
            used_tokens += estimate_message_tokens(selected)

    history_messages = [
        selected_by_id[item.message_id]
        for item in history_items
        if item.message_id in selected_by_id
    ]
    dropped_items = [
        item for item in history_items if item.message_id not in selected_by_id
    ]

    messages: list[BaseMessage] = [system_message]
    if summary_message is not None:
        messages.append(summary_message)
    messages.extend(history_messages)
    if extra_message is not None:
        messages.append(extra_message)

    return BudgetedContext(
        messages=messages,
        dropped_items=dropped_items,
        context_budget=context_budget,
        fallback_input_tokens=sum(estimate_message_tokens(message) for message in messages),
    )


def merge_rolling_summary(
    current_summary: str | None,
    dropped_items: list[ContextHistoryItem],
) -> str | None:
    """Maintain a bounded extractive summary of omitted history."""
    if not dropped_items:
        return current_summary

    parts: list[str] = []
    if current_summary and current_summary.strip():
        parts.append(current_summary.strip())

    omitted_lines = [
        _summarize_history_item(item)
        for item in dropped_items
        if item.raw_content.strip()
    ]
    if omitted_lines:
        parts.append("Omitted earlier conversation:\n" + "\n".join(omitted_lines))

    merged = "\n\n".join(parts).strip()
    if not merged:
        return current_summary
    return truncate_text_to_token_budget(merged, ROLLING_SUMMARY_TOKEN_CAP)


def estimate_message_tokens(message: BaseMessage) -> int:
    return MESSAGE_TOKEN_OVERHEAD + estimate_text_tokens(_message_content_text(message))


def estimate_text_tokens(text: str) -> int:
    """Count tokens with tiktoken when available; fall back to a conservative estimate."""
    if not text:
        return 0
    tiktoken_count = _count_tokens_with_tiktoken(text)
    if tiktoken_count is not None:
        return tiktoken_count
    ascii_chars = sum(1 for char in text if ord(char) < 128)
    non_ascii_chars = len(text) - ascii_chars
    return math.ceil(ascii_chars / 4) + non_ascii_chars


def truncate_text_to_token_budget(text: str, max_tokens: int) -> str:
    if max_tokens <= 0:
        return ""
    if estimate_text_tokens(text) <= max_tokens:
        return text

    marker = "\n...[truncated to fit context budget]...\n"
    marker_tokens = estimate_text_tokens(marker)
    if max_tokens <= marker_tokens + 2:
        return "[truncated]"

    low = 0
    high = len(text)
    best = "[truncated]"
    while low <= high:
        size = (low + high) // 2
        head_size = math.ceil(size / 2)
        tail_size = size - head_size
        tail = text[len(text) - tail_size :] if tail_size else ""
        candidate = f"{text[:head_size]}{marker}{tail}"
        if estimate_text_tokens(candidate) <= max_tokens:
            best = candidate
            low = size + 1
        else:
            high = size - 1
    return best


def fit_message_to_token_budget(message: BaseMessage, max_tokens: int) -> BaseMessage:
    if estimate_message_tokens(message) <= max_tokens:
        return message
    content_budget = max(1, max_tokens - MESSAGE_TOKEN_OVERHEAD)
    return _message_with_content(
        message,
        truncate_text_to_token_budget(_message_content_text(message), content_budget),
    )


def _agent_config(agent: Agent) -> Mapping[str, object]:
    config = agent.llm_config
    if isinstance(config, dict):
        return config
    return {}


def _configured_context_window(config: Mapping[str, object]) -> int | None:
    for key in CONTEXT_WINDOW_CONFIG_KEYS:
        value = config.get(key)
        if isinstance(value, bool):
            continue
        if isinstance(value, int | float):
            parsed = math.floor(value)
            if parsed > 0:
                return parsed
        if isinstance(value, str):
            try:
                parsed = int(value.strip())
            except ValueError:
                continue
            if parsed > 0:
                return parsed
    return None


def _configured_model_name(config: Mapping[str, object]) -> str | None:
    value = config.get("model")
    if isinstance(value, str) and value.strip():
        return value.strip()
    return None


def _configured_output_reserve_ratio(config: Mapping[str, object]) -> float | None:
    for key in OUTPUT_RESERVE_RATIO_CONFIG_KEYS:
        value = config.get(key)
        parsed: float | None = None
        if isinstance(value, bool):
            continue
        if isinstance(value, int | float):
            parsed = float(value)
        elif isinstance(value, str):
            try:
                parsed = float(value.strip())
            except ValueError:
                continue
        if parsed is not None and 0 < parsed < 1:
            return parsed
    return None


def _model_default_context_window(model_name: str | None) -> int:
    if not model_name:
        return DEFAULT_CONTEXT_WINDOW_TOKENS
    lowered = model_name.casefold()
    for fragment, window in MODEL_CONTEXT_WINDOWS:
        if fragment in lowered:
            return window
    return DEFAULT_CONTEXT_WINDOW_TOKENS


def _summary_message(rolling_summary: str | None, available_tokens: int) -> SystemMessage | None:
    if not rolling_summary or not rolling_summary.strip() or available_tokens <= 0:
        return None
    summary_cap = min(SUMMARY_TOKEN_CAP, math.floor(available_tokens * SUMMARY_INPUT_FRACTION_CAP))
    if summary_cap <= MESSAGE_TOKEN_OVERHEAD:
        return None
    content_budget = summary_cap - MESSAGE_TOKEN_OVERHEAD
    summary_text = truncate_text_to_token_budget(rolling_summary.strip(), content_budget)
    return SystemMessage(content=f"Conversation summary so far:\n{summary_text}")


def _summarize_history_item(item: ContextHistoryItem) -> str:
    content = item.raw_content.strip()
    compact_content = truncate_text_to_token_budget(content, SUMMARY_ITEM_TOKEN_CAP)
    return f"- {item.sender_label}: {compact_content}"


def _message_content_text(message: BaseMessage) -> str:
    content = message.content
    if isinstance(content, str):
        return content
    return str(content)


_TIKTOKEN_ENCODING_NAME = "cl100k_base"


@lru_cache(maxsize=1)
def _tiktoken_encoder() -> object | None:
    """Resolve the tiktoken encoder once, or return ``None`` when it is unusable.

    tiktoken may be absent, or installed but unable to materialise the encoding:
    in a PyInstaller bundle the ``tiktoken_ext`` namespace package is not
    discoverable and ``get_encoding`` raises ``Unknown encoding cl100k_base.
    Plugins found: []``; offline, the lazy vocabulary download fails. In every
    such case we degrade to the conservative character estimate rather than let
    token accounting crash the request, so we catch broadly by design.
    """
    try:
        module = import_module("tiktoken")
    except ModuleNotFoundError:
        return None
    if not isinstance(module, ModuleType):
        return None
    get_encoding = getattr(module, "get_encoding", None)
    if not callable(get_encoding):
        return None
    try:
        encoding: object = get_encoding(_TIKTOKEN_ENCODING_NAME)
    except Exception:
        return None
    if not callable(getattr(encoding, "encode", None)):
        return None
    return encoding


def _count_tokens_with_tiktoken(text: str) -> int | None:
    encoding = _tiktoken_encoder()
    if encoding is None:
        return None
    encode = getattr(encoding, "encode", None)
    if not callable(encode):
        return None
    try:
        result = encode(text)
    except Exception:
        return None
    return len(result)


def _message_with_content(message: BaseMessage, content: str) -> BaseMessage:
    if isinstance(message, SystemMessage):
        return SystemMessage(content=content)
    if isinstance(message, AIMessage):
        return AIMessage(content=content)
    return HumanMessage(content=content)
