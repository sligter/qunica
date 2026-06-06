"""LLM provider service — owner-scoped CRUD.

The `kind` value is one of:
- 'openai-compatible' (covers OpenAI, DeepSeek, Qwen, MiMo, Together,
  OpenRouter, Gemini's OpenAI-compat mode, etc.)
- 'anthropic' (Claude direct via langchain_anthropic)

API responses mask `api_key`; internal use (chat-model factory) reads it
plain via `get_for_use`.
"""

from uuid import UUID

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.exceptions import NotFoundError, PermissionDeniedError
from app.models.llm_provider import LLMProvider
from app.models.user import User
from app.schemas.llm_provider import LLMProviderCreate, LLMProviderUpdate

VALID_KINDS = ("openai-compatible", "anthropic", "anthropic-compatible", "gemini")


def mask_api_key(api_key: str) -> str:
    if len(api_key) <= 4:
        return "…"
    return f"…{api_key[-4:]}"


async def create_provider(
    db: AsyncSession, data: LLMProviderCreate, owner: User
) -> LLMProvider:
    if data.kind not in VALID_KINDS:
        from app.core.exceptions import AgentChatError

        raise AgentChatError(f"unsupported provider kind: {data.kind}")
    provider = LLMProvider(
        owner_id=owner.id,
        name=data.name,
        kind=data.kind,
        base_url=data.base_url,
        api_key=data.api_key,
        default_model=data.default_model,
        description=data.description,
        reasoning_passback=data.reasoning_passback,
    )
    db.add(provider)
    await db.flush()
    await db.refresh(provider)
    return provider


async def list_providers(db: AsyncSession, owner: User) -> list[LLMProvider]:
    stmt = (
        select(LLMProvider)
        .where(LLMProvider.owner_id == owner.id, LLMProvider.status == "active")
        .order_by(LLMProvider.created_at.desc())
    )
    return list(await db.scalars(stmt))


async def get_provider(
    db: AsyncSession, provider_id: UUID, owner: User
) -> LLMProvider:
    provider = await db.scalar(
        select(LLMProvider).where(LLMProvider.id == provider_id)
    )
    if provider is None:
        raise NotFoundError(f"provider {provider_id}")
    if provider.owner_id != owner.id:
        raise PermissionDeniedError("provider not accessible")
    return provider


async def get_for_use(db: AsyncSession, provider_id: UUID) -> LLMProvider:
    """Internal: fetch a provider for chat-model construction. Skips owner
    check (caller has already validated agent ownership)."""
    provider = await db.scalar(
        select(LLMProvider).where(LLMProvider.id == provider_id)
    )
    if provider is None:
        raise NotFoundError(f"provider {provider_id}")
    return provider


async def update_provider(
    db: AsyncSession,
    provider_id: UUID,
    data: LLMProviderUpdate,
    owner: User,
) -> LLMProvider:
    provider = await get_provider(db, provider_id, owner)
    if data.name is not None:
        provider.name = data.name
    if data.kind is not None:
        if data.kind not in VALID_KINDS:
            from app.core.exceptions import AgentChatError

            raise AgentChatError(f"unsupported provider kind: {data.kind}")
        provider.kind = data.kind
    if "base_url" in data.model_fields_set:
        provider.base_url = data.base_url
    if data.api_key:
        provider.api_key = data.api_key
    if data.default_model is not None:
        provider.default_model = data.default_model
    if "description" in data.model_fields_set:
        provider.description = data.description
    if data.reasoning_passback is not None:
        provider.reasoning_passback = data.reasoning_passback
    await db.flush()
    await db.refresh(provider)
    return provider


async def delete_provider(
    db: AsyncSession, provider_id: UUID, owner: User
) -> None:
    provider = await get_provider(db, provider_id, owner)
    # Soft-delete: flip status; mirrors how Group/Agent are tombstoned.
    # Agents pointing at this provider continue working until reassigned;
    # the chat-model factory will simply use the (now hidden) record's
    # api_key to make calls. Acceptable for V1.
    provider.status = "deleted"
    await db.flush()


async def list_models(
    db: AsyncSession, provider_id: UUID, owner: User
) -> list[dict[str, str]]:
    """Fetch available models from the provider's API."""
    provider = await get_provider(db, provider_id, owner)
    return await _fetch_provider_models(provider)


async def _fetch_provider_models(provider: LLMProvider) -> list[dict[str, str]]:
    """Fetch available models from one provider record."""
    import httpx

    models: list[dict[str, str]] = []

    try:
        async with httpx.AsyncClient(timeout=15.0) as client:
            if provider.kind == "openai-compatible":
                base = (provider.base_url or "https://api.openai.com/v1").rstrip("/")
                resp = await client.get(
                    f"{base}/models",
                    headers={"Authorization": f"Bearer {provider.api_key}"},
                )
                resp.raise_for_status()
                data = resp.json().get("data", [])
                models = [{"id": m["id"], "name": m.get("id", "")} for m in data]

            elif provider.kind in {"anthropic", "anthropic-compatible"}:
                base = (provider.base_url or "https://api.anthropic.com").rstrip("/")
                resp = await client.get(
                    f"{base}/v1/models",
                    headers={
                        "x-api-key": provider.api_key,
                        "anthropic-version": "2023-06-01",
                    },
                )
                resp.raise_for_status()
                data = resp.json().get("data", [])
                models = [{"id": m["id"], "name": m.get("display_name", m["id"])} for m in data]

            elif provider.kind == "gemini":
                base = (provider.base_url or "https://generativelanguage.googleapis.com/v1beta").rstrip("/")
                resp = await client.get(
                    f"{base}/models",
                    params={"key": provider.api_key},
                )
                resp.raise_for_status()
                data = resp.json().get("models", [])
                models = [
                    {
                        "id": m.get("name", "").replace("models/", ""),
                        "name": m.get("displayName", m.get("name", "")),
                    }
                    for m in data
                ]
    except Exception:
        # Return empty list on failure — frontend handles gracefully
        pass

    return models
