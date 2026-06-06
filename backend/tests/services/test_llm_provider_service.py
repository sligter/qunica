from typing import Any
from uuid import uuid4

import httpx
import pytest
from pydantic import ValidationError

from app.models.llm_provider import LLMProvider
from app.schemas.llm_provider import LLMProviderCreate
from app.services.llm_provider_service import VALID_KINDS, _fetch_provider_models


def test_anthropic_compatible_provider_kind_is_valid() -> None:
    data = LLMProviderCreate(
        name="Claude gateway",
        kind="anthropic-compatible",
        base_url="https://gateway.example",
        api_key="sk-ant-test",
        default_model="claude-sonnet-4-5",
    )

    assert data.kind == "anthropic-compatible"
    assert "anthropic-compatible" in VALID_KINDS


def test_unknown_provider_kind_is_rejected() -> None:
    with pytest.raises(ValidationError):
        LLMProviderCreate(
            name="Bad gateway",
            kind="anthropic_compat",
            api_key="sk-test",
            default_model="model",
        )


@pytest.mark.asyncio
async def test_anthropic_compatible_model_list_uses_anthropic_wire_format(
    monkeypatch: Any,
) -> None:
    provider = LLMProvider(
        owner_id=uuid4(),
        name="Claude gateway",
        kind="anthropic-compatible",
        base_url="https://gateway.example",
        api_key="sk-ant-test",
        default_model="claude-sonnet-4-5",
    )
    calls: list[dict[str, Any]] = []

    class FakeResponse:
        def raise_for_status(self) -> None:
            return None

        def json(self) -> dict[str, list[dict[str, str]]]:
            return {
                "data": [
                    {"id": "claude-sonnet-4-5", "display_name": "Claude Sonnet 4.5"}
                ]
            }

    class FakeAsyncClient:
        def __init__(self, *args: Any, **kwargs: Any) -> None:
            _ = args, kwargs

        async def __aenter__(self) -> "FakeAsyncClient":
            return self

        async def __aexit__(self, *args: Any) -> None:
            _ = args

        async def get(
            self,
            url: str,
            *,
            headers: dict[str, str] | None = None,
            params: dict[str, str] | None = None,
        ) -> FakeResponse:
            calls.append({"url": url, "headers": headers, "params": params})
            return FakeResponse()

    monkeypatch.setattr(httpx, "AsyncClient", FakeAsyncClient)

    models = await _fetch_provider_models(provider)

    assert models == [{"id": "claude-sonnet-4-5", "name": "Claude Sonnet 4.5"}]
    assert calls == [
        {
            "url": "https://gateway.example/v1/models",
            "headers": {
                "x-api-key": "sk-ant-test",
                "anthropic-version": "2023-06-01",
            },
            "params": None,
        }
    ]
