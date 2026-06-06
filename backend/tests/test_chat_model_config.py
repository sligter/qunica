from langchain_core.messages import AIMessageChunk

from app.llm.chat_model import ReasoningChatOpenAI, _provider_reasoning_params


def _model() -> ReasoningChatOpenAI:
    # A dummy key + unreachable base_url keeps construction offline; the
    # conversion methods under test never make a network call.
    return ReasoningChatOpenAI(
        model="deepseek-reasoner",
        api_key="test-key",
        base_url="https://example.invalid/v1",
    )


def test_reasoning_effort_maps_to_provider_specific_kwargs() -> None:
    cfg = {"reasoning_effort": "high"}

    assert _provider_reasoning_params("openai-compatible", cfg) == {
        "reasoning_effort": "high"
    }
    assert _provider_reasoning_params("anthropic", cfg) == {"effort": "high"}
    assert _provider_reasoning_params("anthropic-compatible", cfg) == {"effort": "high"}
    assert _provider_reasoning_params("gemini", cfg) == {"thinking_level": "high"}


def test_xhigh_reasoning_effort_is_accepted() -> None:
    cfg = {"reasoning_effort": "xhigh"}

    # `xhigh` is a first-class tier now (langchain-anthropic natively supports
    # it; OpenAI-compatible providers receive it verbatim).
    assert _provider_reasoning_params("openai-compatible", cfg) == {
        "reasoning_effort": "xhigh"
    }
    assert _provider_reasoning_params("anthropic", cfg) == {"effort": "xhigh"}
    assert _provider_reasoning_params("anthropic-compatible", cfg) == {"effort": "xhigh"}


def test_reasoning_effort_ignores_default_and_invalid_values() -> None:
    assert _provider_reasoning_params("openai-compatible", {}) == {}
    assert _provider_reasoning_params("openai-compatible", {"reasoning_effort": "default"}) == {}
    assert _provider_reasoning_params("openai-compatible", {"reasoning_effort": "extreme"}) == {}


def test_reasoning_chat_openai_preserves_streaming_reasoning_content() -> None:
    chunk = {
        "choices": [
            {
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "content": "",
                    "reasoning_content": "let me think",
                },
                "finish_reason": None,
            }
        ]
    }

    generation = _model()._convert_chunk_to_generation_chunk(chunk, AIMessageChunk, {})

    assert generation is not None
    assert generation.message.additional_kwargs.get("reasoning_content") == "let me think"


def test_reasoning_chat_openai_supports_reasoning_alias() -> None:
    # OpenRouter and some gateways expose the field as `reasoning`, not
    # `reasoning_content`.
    chunk = {
        "choices": [
            {"index": 0, "delta": {"reasoning": "step one"}, "finish_reason": None}
        ]
    }

    generation = _model()._convert_chunk_to_generation_chunk(chunk, AIMessageChunk, {})

    assert generation is not None
    assert generation.message.additional_kwargs.get("reasoning_content") == "step one"


def test_reasoning_chat_openai_leaves_plain_chunks_untouched() -> None:
    chunk = {"choices": [{"index": 0, "delta": {"content": "hi"}, "finish_reason": None}]}

    generation = _model()._convert_chunk_to_generation_chunk(chunk, AIMessageChunk, {})

    assert generation is not None
    assert "reasoning_content" not in generation.message.additional_kwargs


def test_reasoning_chat_openai_preserves_final_reasoning_content() -> None:
    response = {
        "id": "cmpl-1",
        "model": "deepseek-reasoner",
        "choices": [
            {
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "the answer",
                    "reasoning_content": "the thoughts",
                },
                "finish_reason": "stop",
            }
        ],
    }

    result = _model()._create_chat_result(response)

    message = result.generations[0].message
    assert message.content == "the answer"
    assert message.additional_kwargs.get("reasoning_content") == "the thoughts"
