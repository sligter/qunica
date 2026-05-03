"""Skill service — owner-scoped CRUD plus SKILL.md import.

Anthropic skill format (V1 subset): a SKILL.md file beginning with YAML
frontmatter `{name, description}`, followed by the markdown body. Scripts,
references, and other files are not yet supported.

The body is appended verbatim to an agent's system prompt when the skill is
mounted on the agent — see `message_service._build_lc_input`.
"""

from uuid import UUID

import yaml  # type: ignore[import-untyped]
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.exceptions import (
    AgentChatError,
    NotFoundError,
    PermissionDeniedError,
)
from app.models.skill import Skill
from app.models.user import User
from app.schemas.skill import SkillCreate


def _parse_skill_md(raw: str) -> tuple[str, str | None, str]:
    """Return `(name, description, body_markdown)`.

    Raises `AgentChatError` if the input doesn't look like a valid SKILL.md.
    """
    text = raw.lstrip()
    if not text.startswith("---\n") and not text.startswith("---\r\n"):
        raise AgentChatError("SKILL.md must start with YAML frontmatter (---)")
    # Tolerant to CRLF.
    body_open = text.find("\n", 4)
    end = text.find("\n---\n", body_open)
    if end == -1:
        end = text.find("\n---\r\n", body_open)
    if end == -1:
        raise AgentChatError("SKILL.md frontmatter is not closed (missing `---`)")
    fm_text = text[4:end]
    try:
        fm = yaml.safe_load(fm_text) or {}
    except yaml.YAMLError as exc:
        raise AgentChatError(f"SKILL.md frontmatter is not valid YAML: {exc}") from exc
    if not isinstance(fm, dict):
        raise AgentChatError("SKILL.md frontmatter must be a YAML mapping")
    name = fm.get("name")
    if not name or not isinstance(name, str):
        raise AgentChatError("SKILL.md frontmatter must include `name` (string)")
    description_raw = fm.get("description")
    description = (
        str(description_raw) if description_raw is not None else None
    )
    # Skip past the closing `---\n` line.
    after = text[end:].lstrip("\n")
    if after.startswith("---"):
        # Drop the literal `---` line + any trailing newline.
        nl = after.find("\n")
        after = after[nl + 1 :] if nl != -1 else ""
    body = after.lstrip("\n")
    return name, description, body


async def create_skill(
    db: AsyncSession, data: SkillCreate, owner: User, source: str = "manual"
) -> Skill:
    skill = Skill(
        owner_id=owner.id,
        name=data.name,
        description=data.description,
        body_markdown=data.body_markdown,
        source=source,
    )
    db.add(skill)
    await db.flush()
    await db.refresh(skill)
    return skill


async def import_skill_from_md(
    db: AsyncSession, raw: str, owner: User
) -> Skill:
    name, description, body = _parse_skill_md(raw)
    return await create_skill(
        db,
        SkillCreate(name=name, description=description, body_markdown=body),
        owner,
        source="imported",
    )


async def list_skills(db: AsyncSession, owner: User) -> list[Skill]:
    stmt = (
        select(Skill)
        .where(Skill.owner_id == owner.id, Skill.status == "active")
        .order_by(Skill.created_at.desc())
    )
    return list(await db.scalars(stmt))


async def get_skill(db: AsyncSession, skill_id: UUID, owner: User) -> Skill:
    skill = await db.scalar(select(Skill).where(Skill.id == skill_id))
    if skill is None:
        raise NotFoundError(f"skill {skill_id}")
    if skill.owner_id != owner.id:
        raise PermissionDeniedError("skill not accessible")
    return skill


async def list_by_ids(
    db: AsyncSession, skill_ids: list[UUID]
) -> list[Skill]:
    """Internal: fetch skills referenced by an agent for prompt assembly.

    Skips owner check — caller (message_service) has already validated agent
    ownership; the agent's skill_ids list is itself authoritative because
    only the owner can mount skills. Soft-deleted skills (`status='deleted'`)
    are excluded so the prompt doesn't leak revoked content.
    """
    if not skill_ids:
        return []
    stmt = select(Skill).where(
        Skill.id.in_(skill_ids), Skill.status == "active"
    )
    rows = list(await db.scalars(stmt))
    # Preserve order from skill_ids for determinism.
    by_id = {s.id: s for s in rows}
    return [by_id[i] for i in skill_ids if i in by_id]


async def delete_skill(db: AsyncSession, skill_id: UUID, owner: User) -> None:
    skill = await get_skill(db, skill_id, owner)
    skill.status = "deleted"
    await db.flush()
