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

VALID_KINDS = ("openai-compatible", "anthropic")


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
    if data.base_url is not None:
        provider.base_url = data.base_url
    if data.api_key is not None:
        provider.api_key = data.api_key
    if data.default_model is not None:
        provider.default_model = data.default_model
    if data.description is not None:
        provider.description = data.description
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
