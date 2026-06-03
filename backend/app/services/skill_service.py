"""Skill service — owner-scoped CRUD plus SKILL.md/package import."""

import io
import os
import re
import zipfile
from dataclasses import dataclass
from pathlib import Path, PurePosixPath, PureWindowsPath
from typing import Any
from uuid import UUID

import yaml  # type: ignore[import-untyped]
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.config import PROJECT_ROOT
from app.core.exceptions import (
    AgentChatError,
    NotFoundError,
    PermissionDeniedError,
)
from app.models.skill import Skill
from app.models.user import User
from app.schemas.skill import SkillCreate
from app.services import system_settings_service

_FRONTMATTER_RE = re.compile(
    r"\A(?:﻿)?[ \t]*---[ \t]*\r?\n(.*?)\r?\n---[ \t]*(?:\r?\n|\Z)",
    re.DOTALL,
)
_CLASSIFIED_DIRS = {"references", "scripts", "assets", "tools"}
_TEXT_EXTENSIONS = {
    ".md",
    ".markdown",
    ".txt",
    ".json",
    ".yaml",
    ".yml",
    ".toml",
    ".xml",
    ".html",
    ".css",
    ".js",
    ".jsx",
    ".ts",
    ".tsx",
    ".py",
    ".sh",
    ".bash",
    ".zsh",
    ".fish",
    ".ps1",
    ".sql",
    ".csv",
    ".ini",
    ".cfg",
}
_MAX_TEXT_FILE_BYTES = 1_000_000


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


def _normalize_resource_path(path: str) -> str:
    normalized = path.replace("\\", "/")
    if _zip_entry_is_unsafe(normalized) or normalized.endswith("/") or not normalized:
        raise AgentChatError("resource path is not allowed")
    return normalized


def _file_info_for_path(skill: Skill, rel_path: str) -> dict[str, object]:
    for info in skill.files or []:
        if info.get("path") == rel_path:
            return dict(info)
    raise NotFoundError(f"skill resource {rel_path}")


def _skill_storage_dir(skill: Skill) -> Path:
    if not skill.storage_path:
        raise NotFoundError(f"skill resources {skill.id}")
    path = Path(skill.storage_path)
    if not path.is_absolute():
        path = PROJECT_ROOT / path
    return path.resolve()


def _resource_file_path(skill: Skill, rel_path: str) -> Path:
    storage_dir = _skill_storage_dir(skill)
    target = (storage_dir / rel_path).resolve()
    try:
        if os.path.commonpath([str(storage_dir), str(target)]) != str(storage_dir):
            raise AgentChatError("resource path is not allowed")
    except ValueError as exc:
        raise AgentChatError("resource path is not allowed") from exc
    if not target.is_file():
        raise NotFoundError(f"skill resource {rel_path}")
    return target


def _is_text_resource(path: Path, size: int) -> bool:
    if size > _MAX_TEXT_FILE_BYTES or path.suffix.lower() not in _TEXT_EXTENSIONS:
        return False
    try:
        path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        return False
    return True


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


async def list_by_ids(
    db: AsyncSession,
    skill_ids: list[UUID],
    *,
    owner: User,
) -> list[Skill]:
    """Internal: fetch skills referenced by an agent for prompt assembly."""
    if not skill_ids:
        return []
    stmt = select(Skill).where(
        Skill.id.in_(skill_ids),
        Skill.owner_id == owner.id,
        Skill.status == "active",
    )
    rows = list(await db.scalars(stmt))
    by_id = {s.id: s for s in rows}
    return [by_id[i] for i in skill_ids if i in by_id]


async def _resolve_skill_storage_dir(
    db: AsyncSession, owner: User, skill_id: UUID
) -> Path:
    """Resolve the on-disk storage location for a skill package.

    Preference order:
    1. `<system_settings.group_workspace_root>/skillshub/<skill_id>` so users
       can keep skill packages alongside their group workspaces.
    2. `<backend_project_root>/uploads/skills/<skill_id>` as a fallback when no
       global root is configured yet.
    """
    settings = await system_settings_service.get_or_create(db, owner)
    if settings.group_workspace_root:
        base = Path(settings.group_workspace_root) / "skillshub" / str(skill_id)
    else:
        base = PROJECT_ROOT / "uploads" / "skills" / str(skill_id)
    base.mkdir(parents=True, exist_ok=True)
    return base.resolve()


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

    storage_dir = await _resolve_skill_storage_dir(db, owner, skill.id)
    for rel, payload in file_payloads:
        target = storage_dir / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(payload)

    skill.storage_path = str(storage_dir)
    await db.flush()
    await db.refresh(skill)
    return skill


async def list_skill_resources(
    db: AsyncSession, skill_id: UUID, owner: User
) -> list[dict[str, object]]:
    skill = await get_skill(db, skill_id, owner)
    resources: list[dict[str, object]] = []
    for info in skill.files or []:
        rel_path = str(info.get("path", ""))
        if not rel_path:
            continue
        try:
            path = _resource_file_path(skill, _normalize_resource_path(rel_path))
        except (AgentChatError, NotFoundError):
            continue
        size = path.stat().st_size
        resources.append(
            {
                "path": rel_path,
                "size": size,
                "category": str(info.get("category", "other")),
                "is_text": _is_text_resource(path, size),
                "content": None,
            }
        )
    return resources


async def read_skill_resource(
    db: AsyncSession, skill_id: UUID, rel_path: str, owner: User
) -> dict[str, object]:
    skill = await get_skill(db, skill_id, owner)
    normalized = _normalize_resource_path(rel_path)
    info = _file_info_for_path(skill, normalized)
    path = _resource_file_path(skill, normalized)
    size = path.stat().st_size
    is_text = _is_text_resource(path, size)
    content: str | None = None
    if is_text:
        try:
            content = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            is_text = False
    return {
        "path": normalized,
        "size": size,
        "category": str(info.get("category", "other")),
        "is_text": is_text,
        "content": content,
    }


async def update_skill_resource(
    db: AsyncSession, skill_id: UUID, rel_path: str, content: str, owner: User
) -> dict[str, object]:
    skill = await get_skill(db, skill_id, owner)
    normalized = _normalize_resource_path(rel_path)
    info = _file_info_for_path(skill, normalized)
    path = _resource_file_path(skill, normalized)
    current_size = path.stat().st_size
    if not _is_text_resource(path, current_size):
        raise AgentChatError("resource is not editable as text")
    try:
        path.read_text(encoding="utf-8")
    except UnicodeDecodeError as exc:
        raise AgentChatError("resource is not editable as text") from exc
    path.write_text(content, encoding="utf-8")
    new_size = path.stat().st_size
    for item in skill.files or []:
        if item.get("path") == normalized:
            item["size"] = new_size
            break
    skill.files = list(skill.files or [])
    await db.flush()
    return {
        "path": normalized,
        "size": new_size,
        "category": str(info.get("category", "other")),
        "is_text": True,
        "content": content,
    }


async def delete_skill(db: AsyncSession, skill_id: UUID, owner: User) -> None:
    skill = await get_skill(db, skill_id, owner)
    skill.status = "deleted"
    await db.flush()
