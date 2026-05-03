from uuid import UUID

from fastapi import APIRouter, Depends, status
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.deps import get_current_user
from app.db import get_db
from app.models.user import User
from app.schemas.llm_provider import (
    LLMProviderCreate,
    LLMProviderRead,
    LLMProviderUpdate,
)
from app.services import llm_provider_service
from app.services.llm_provider_service import mask_api_key

router = APIRouter(prefix="/llm-providers", tags=["llm-providers"])


def _to_read(provider: object) -> LLMProviderRead:
    p = provider  # ORM instance
    return LLMProviderRead(
        id=p.id,  # type: ignore[attr-defined]
        name=p.name,  # type: ignore[attr-defined]
        kind=p.kind,  # type: ignore[attr-defined]
        base_url=p.base_url,  # type: ignore[attr-defined]
        api_key_masked=mask_api_key(p.api_key),  # type: ignore[attr-defined]
        default_model=p.default_model,  # type: ignore[attr-defined]
        description=p.description,  # type: ignore[attr-defined]
        status=p.status,  # type: ignore[attr-defined]
        created_at=p.created_at,  # type: ignore[attr-defined]
    )


@router.post(
    "", response_model=LLMProviderRead, status_code=status.HTTP_201_CREATED
)
async def create_provider(
    data: LLMProviderCreate,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> LLMProviderRead:
    provider = await llm_provider_service.create_provider(db, data, current_user)
    return _to_read(provider)


@router.get("", response_model=list[LLMProviderRead])
async def list_providers(
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> list[LLMProviderRead]:
    rows = await llm_provider_service.list_providers(db, current_user)
    return [_to_read(p) for p in rows]


@router.get("/{provider_id}", response_model=LLMProviderRead)
async def get_provider(
    provider_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> LLMProviderRead:
    provider = await llm_provider_service.get_provider(db, provider_id, current_user)
    return _to_read(provider)


@router.patch("/{provider_id}", response_model=LLMProviderRead)
async def update_provider(
    provider_id: UUID,
    data: LLMProviderUpdate,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> LLMProviderRead:
    provider = await llm_provider_service.update_provider(
        db, provider_id, data, current_user
    )
    return _to_read(provider)


@router.get("/{provider_id}/models")
async def list_provider_models(
    provider_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> list[dict[str, str]]:
    return await llm_provider_service.list_models(db, provider_id, current_user)


@router.delete("/{provider_id}", status_code=status.HTTP_204_NO_CONTENT)
async def delete_provider(
    provider_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> None:
    await llm_provider_service.delete_provider(db, provider_id, current_user)
