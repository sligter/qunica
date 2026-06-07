from collections.abc import Sequence
from datetime import UTC, datetime
from importlib import import_module
from types import ModuleType
from uuid import UUID, uuid4

import pytest
from langchain_core.messages import HumanMessage, SystemMessage

import app.agents.context_budget as context_budget
from app.agents.context_budget import (
    ContextBudget,
    ContextHistoryItem,
    build_budgeted_context,
    merge_rolling_summary,
    resolve_context_budget,
)
from app.models.agent import Agent


def _agent(llm_config: dict[str, object] | None = None) -> Agent:
    return Agent(
        owner_id=uuid4(),
        name="Budget Agent",
        system_prompt="System prompt",
        llm_config=llm_config,
    )


def _history_item(message_id: UUID, content: str, label: str = "User") -> ContextHistoryItem:
    return ContextHistoryItem(
        message_id=message_id,
        created_at=datetime.now(UTC),
        sender_label=label,
        message=HumanMessage(content=f"[{label}]: {content}"),
        raw_content=content,
    )


def _content(messages: Sequence[object]) -> str:
    return "\n".join(str(getattr(message, "content", "")) for message in messages)


def test_resolve_context_budget_uses_agent_config_and_reserves_output() -> None:
    budget = resolve_context_budget(_agent({"context_window_tokens": 1000}))

    assert budget.context_window_tokens == 1000
    assert budget.output_reserve_tokens == 300
    assert budget.input_budget_tokens == 700


def test_resolve_context_budget_uses_model_defaults() -> None:
    budget = resolve_context_budget(_agent({"model": "gpt-4o-mini"}))

    assert budget.context_window_tokens == 128_000
    assert budget.output_reserve_tokens == 38_400
    assert budget.input_budget_tokens == 89_600


def test_budget_keeps_required_message_and_recent_history() -> None:
    required_id = uuid4()
    history_items = [
        _history_item(uuid4(), "old " * 400),
        _history_item(required_id, "must keep"),
        _history_item(uuid4(), "recent one"),
        _history_item(uuid4(), "recent two"),
    ]

    budgeted = build_budgeted_context(
        system_message=SystemMessage(content="system"),
        history_items=history_items,
        rolling_summary=None,
        context_budget=ContextBudget(
            context_window_tokens=90,
            output_reserve_tokens=27,
            input_budget_tokens=63,
        ),
        required_message_id=required_id,
    )
    content = _content(budgeted.messages)

    assert "must keep" in content
    assert "recent two" in content
    assert "old old old" not in content
    assert history_items[0] in budgeted.dropped_items


def test_budget_truncates_overlong_required_message() -> None:
    required_id = uuid4()
    history_items = [_history_item(required_id, "x " * 1000)]

    budgeted = build_budgeted_context(
        system_message=SystemMessage(content="system"),
        history_items=history_items,
        rolling_summary=None,
        context_budget=ContextBudget(
            context_window_tokens=80,
            output_reserve_tokens=24,
            input_budget_tokens=56,
        ),
        required_message_id=required_id,
    )

    assert "truncated to fit context budget" in _content(budgeted.messages)
    assert not budgeted.dropped_items


def test_budget_includes_existing_rolling_summary() -> None:
    budgeted = build_budgeted_context(
        system_message=SystemMessage(content="system"),
        history_items=[],
        rolling_summary="Earlier: Alice asked for a report.",
        context_budget=ContextBudget(
            context_window_tokens=1000,
            output_reserve_tokens=300,
            input_budget_tokens=700,
        ),
    )

    assert len(budgeted.messages) == 2
    assert "Conversation summary so far" in str(budgeted.messages[1].content)
    assert "Alice asked for a report" in str(budgeted.messages[1].content)


def test_merge_rolling_summary_appends_dropped_history() -> None:
    item = _history_item(uuid4(), "New omitted detail", label="Alice")

    summary = merge_rolling_summary("Existing summary", [item])

    assert summary is not None
    assert "Existing summary" in summary
    assert "Alice" in summary
    assert "New omitted detail" in summary


def test_tiktoken_encoder_degrades_on_unknown_encoding(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A bundled tiktoken that cannot load cl100k_base must not propagate.

    Reproduces ``ValueError: Unknown encoding cl100k_base. Plugins found: []``
    raised inside PyInstaller builds where the ``tiktoken_ext`` namespace
    package is not discoverable.
    """
    fake_tiktoken = ModuleType("tiktoken")

    def _raise_unknown(name: str) -> object:
        raise ValueError(f"Unknown encoding {name}. Plugins found: []")

    fake_tiktoken.__dict__["get_encoding"] = _raise_unknown

    def _fake_import(name: str) -> ModuleType:
        return fake_tiktoken if name == "tiktoken" else import_module(name)

    monkeypatch.setattr(context_budget, "import_module", _fake_import)
    context_budget._tiktoken_encoder.cache_clear()
    try:
        assert context_budget._tiktoken_encoder() is None
        assert context_budget.estimate_text_tokens("hello world") > 0
    finally:
        context_budget._tiktoken_encoder.cache_clear()


def test_estimate_text_tokens_falls_back_when_encode_raises(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """An encoder that resolves but fails mid-encode must still yield an estimate."""

    class _BoomEncoder:
        def encode(self, text: str) -> list[int]:
            raise RuntimeError("boom")

    monkeypatch.setattr(context_budget, "_tiktoken_encoder", lambda: _BoomEncoder())

    assert context_budget.estimate_text_tokens("hello world") > 0
