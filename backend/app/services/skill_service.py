"""Skill service — owner-scoped CRUD plus SKILL.md/package import."""

import io
import re
import zipfile
from dataclasses import dataclass
from pathlib import Path, PurePosixPath, PureWindowsPath
from typing import Any
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

_FRONTMATTER_RE = re.compile(
    r"\A(?:﻿)?[ \t]*---[ \t]*\r?\n(.*?)\r?\n---[ \t]*(?:\r?\n|\Z)",
    re.DOTALL,
)
_CLASSIFIED_DIRS = {"references", "scripts", "assets", "tools"}


@dataclass(frozen=True, slots=True)
class ParsedSkillMarkdown:
    name: str
    description: str | None
    body_markdown: str
    metadata: dict[str, Any]


def _parse_skill_md(raw: str) -> ParsedSkillMarkdown:
    """Parse SKILL.md, preserving frontmatter metadata and body separately.

    Frontmatter is required for imported skills because it provides the stable
    skill name. When present, all metadata keys are preserved as JSON-compatible
    values while the markdown instructions remain in `body_markdown` only.
    """

    match = _FRONTMATTER_RE.match(raw)
    if match is None:
        raise AgentChatError("SKILL.md must start with YAML frontmatter (---)")
    fm_text = match.group(1)
    try:
        fm = yaml.safe_load(fm_text) or {}
    except yaml.YAMLError as exc:
        raise AgentChatError(f"SKILL.md frontmatter is not valid YAML: {exc}") from exc
    if not isinstance(fm, dict):
        raise AgentChatError("SKILL.md frontmatter must be a YAML mapping")

    metadata = _json_safe_metadata(fm)
    name = metadata.get("name")
    if not name or not isinstance(name, str):
        raise AgentChatError("SKILL.md frontmatter must include `name` (string)")
    description_raw = metadata.get("description")
    description = str(description_raw) if description_raw is not None else None
    body = raw[match.end() :].lstrip("\r\n")
    return ParsedSkillMarkdown(
        name=name,
        description=description,
        body_markdown=body,
        metadata=metadata,
    )


def _json_safe_metadata(metadata: dict[Any, Any]) -> dict[str, Any]:
    out: dict[str, Any] = {}
    for key, value in metadata.items():
        if not isinstance(key, str):
            continue
        out[key] = _json_safe_value(value)
    return out


def _json_safe_value(value: Any) -> Any:
    if value is None or isinstance(value, str | int | float | bool):
        return value
    if isinstance(value, list):
        return [_json_safe_value(item) for item in value]
    if isinstance(value, dict):
        return {str(k): _json_safe_value(v) for k, v in value.items()}
    return str(value)


def _zip_entry_is_unsafe(name: str) -> bool:
    normalized = name.replace("\\", "/")
    if normalized.startswith("/") or normalized.startswith("//"):
        return True
    if PureWindowsPath(name).drive:
        return True
    parts = PurePosixPath(normalized).parts
    return any(part == ".." for part in parts)


def _validate_zip_names(names: list[str]) -> None:
    unsafe = [name for name in names if _zip_entry_is_unsafe(name)]
    if unsafe:
        raise AgentChatError(f"zip package contains unsafe path: {unsafe[0]}")


def _classify_zip_entry(rel_path: str) -> str:
    if rel_path == "SKILL.md":
        return "SKILL.md"
    first = rel_path.split("/", 1)[0]
    if first in _CLASSIFIED_DIRS:
        return first
    return "other"


def _find_skill_md_path(names: list[str]) -> tuple[str, str]:
    for name in names:
        if name.endswith("/"):
            continue
        parts = PurePosixPath(name.replace("\\", "/")).parts
        if not parts or parts[-1] != "SKILL.md":
            continue
        if len(parts) <= 2:
            prefix = "" if len(parts) == 1 else f"{parts[0]}/"
            return name, prefix
    raise AgentChatError("zip package must contain a SKILL.md file at root or one level deep")


def _relative_zip_path(name: str, prefix: str) -> str:
    normalized = name.replace("\\", "/")
    return normalized[len(prefix) :] if prefix and normalized.startswith(prefix) else normalized


async def create_skill(
    db: AsyncSession, data: SkillCreate, owner: User, source: str = "manual"
) -> Skill:
    skill = Skill(
        owner_id=owner.id,
        name=data.name,
        description=data.description,
        body_markdown=data.body_markdown,
        metadata_={
            "name": data.name,
            "description": data.description,
        },
        source=source,
    )
    db.add(skill)
    await db.flush()
    await db.refresh(skill)
    return skill


async def import_skill_from_md(db: AsyncSession, raw: str, owner: User) -> Skill:
    parsed = _parse_skill_md(raw)
    skill = Skill(
        owner_id=owner.id,
        name=parsed.name,
        description=parsed.description,
        body_markdown=parsed.body_markdown,
        metadata_=parsed.metadata,
        source="imported",
    )
    db.add(skill)
    await db.flush()
    await db.refresh(skill)
    return skill


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


async def list_by_ids(db: AsyncSession, skill_ids: list[UUID]) -> list[Skill]:
    """Internal: fetch skills referenced by an agent for prompt assembly."""
    if not skill_ids:
        return []
    stmt = select(Skill).where(Skill.id.in_(skill_ids), Skill.status == "active")
    rows = list(await db.scalars(stmt))
    by_id = {s.id: s for s in rows}
    return [by_id[i] for i in skill_ids if i in by_id]


async def import_skill_from_zip(
    db: AsyncSession, file_bytes: bytes, owner: User
) -> Skill:
    """Import a skill from a .zip package with safe path handling."""

    if not zipfile.is_zipfile(io.BytesIO(file_bytes)):
        raise AgentChatError("uploaded file is not a valid zip archive")

    with zipfile.ZipFile(io.BytesIO(file_bytes)) as zf:
        names = zf.namelist()
        _validate_zip_names(names)
        skill_md_path, prefix = _find_skill_md_path(names)
        parsed = _parse_skill_md(zf.read(skill_md_path).decode("utf-8"))

        file_list: list[dict[str, object]] = []
        file_payloads: list[tuple[str, bytes]] = []
        for name in names:
            if name.endswith("/"):
                continue
            rel = _relative_zip_path(name, prefix)
            info = zf.getinfo(name)
            file_list.append(
                {
                    "path": rel,
                    "size": info.file_size,
                    "category": _classify_zip_entry(rel),
                }
            )
            file_payloads.append((rel, zf.read(name)))

    skill = Skill(
        owner_id=owner.id,
        name=parsed.name,
        description=parsed.description,
        body_markdown=parsed.body_markdown,
        metadata_=parsed.metadata,
        source="package",
        files=file_list,
    )
    db.add(skill)
    await db.flush()
    await db.refresh(skill)

    storage_dir = Path("uploads") / "skills" / str(skill.id)
    storage_dir.mkdir(parents=True, exist_ok=True)
    for rel, payload in file_payloads:
        target = storage_dir / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(payload)

    skill.storage_path = str(storage_dir)
    await db.flush()
    await db.refresh(skill)
    return skill


async def delete_skill(db: AsyncSession, skill_id: UUID, owner: User) -> None:
    skill = await get_skill(db, skill_id, owner)
    skill.status = "deleted"
    await db.flush()
