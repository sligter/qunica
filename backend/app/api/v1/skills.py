from uuid import UUID

from fastapi import APIRouter, Depends, UploadFile, status
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.deps import get_current_user
from app.db import get_db
from app.models.user import User
from app.schemas.skill import SkillCreate, SkillImport, SkillRead
from app.services import skill_service

router = APIRouter(prefix="/skills", tags=["skills"])


@router.post("", response_model=SkillRead, status_code=status.HTTP_201_CREATED)
async def create_skill(
    data: SkillCreate,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> SkillRead:
    skill = await skill_service.create_skill(db, data, current_user, source="manual")
    return SkillRead.model_validate(skill)


@router.post(
    "/import", response_model=SkillRead, status_code=status.HTTP_201_CREATED
)
async def import_skill(
    data: SkillImport,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> SkillRead:
    skill = await skill_service.import_skill_from_md(db, data.raw, current_user)
    return SkillRead.model_validate(skill)


@router.post(
    "/import-package", response_model=SkillRead, status_code=status.HTTP_201_CREATED
)
async def import_skill_package(
    file: UploadFile,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> SkillRead:
    content = await file.read()
    skill = await skill_service.import_skill_from_zip(db, content, current_user)
    return SkillRead.model_validate(skill)


@router.get("", response_model=list[SkillRead])
async def list_skills(
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> list[SkillRead]:
    rows = await skill_service.list_skills(db, current_user)
    return [SkillRead.model_validate(s) for s in rows]


@router.get("/{skill_id}", response_model=SkillRead)
async def get_skill(
    skill_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> SkillRead:
    skill = await skill_service.get_skill(db, skill_id, current_user)
    return SkillRead.model_validate(skill)


@router.delete("/{skill_id}", status_code=status.HTTP_204_NO_CONTENT)
async def delete_skill(
    skill_id: UUID,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user),
) -> None:
    await skill_service.delete_skill(db, skill_id, current_user)
