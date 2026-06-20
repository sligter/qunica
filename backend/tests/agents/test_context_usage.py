from app.agents.context_budget import ContextBudget
from app.agents.context_usage import acp_context_usage, fallback_context_usage


def test_acp_context_usage_uses_reported_window_size() -> None:
    fallback = fallback_context_usage(
        input_tokens=1_000,
        context_budget=ContextBudget(
            context_window_tokens=32_000,
            output_reserve_tokens=9_600,
            input_budget_tokens=22_400,
        ),
    )

    usage = acp_context_usage(
        {"used": 42_000, "size": 200_000},
        fallback_usage=fallback,
    )

    assert usage is not None
    assert usage.input_tokens == 42_000
    assert usage.context_window_tokens == 200_000
    assert usage.output_reserve_tokens == 9_600
    assert usage.source == "provider"
    assert usage.ratio == 0.21
