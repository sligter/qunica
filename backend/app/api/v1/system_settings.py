from fastapi import APIRouter, Depends
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.deps import get_current_user
from app.db import get_db
from app.models.system_settings import SystemSettings
from app.models.user import User
from app.schemas.system_settings import SystemSettingsRead, SystemSettingsUpdate
from app.services import system_settings_service

router = APIRouter(prefix="/settings", tags=["settings"])


@router.get("/system", response_model=SystemSettingsRead)
async def read_system_settings(
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> SystemSettings:
    return await system_settings_service.get_or_create(db, current_user)


@router.patch("/system", response_model=SystemSettingsRead)
async def update_system_settings(
    data: SystemSettingsUpdate,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> SystemSettings:
    return await system_settings_service.update(db, current_user, data)
