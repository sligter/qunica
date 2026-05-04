import io
import zipfile
from pathlib import Path
from uuid import uuid4

import pytest
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.exceptions import AgentChatError
from app.models.system_settings import SystemSettings
from app.models.user import User
from app.services import skill_service


async def _user(db: AsyncSession) -> User:
    user = User(email=f"skill-{uuid4().hex[:8]}@example.com", password_hash="x", name="Skill User")
    db.add(user)
    await db.flush()
    return user


async def test_import_skill_md_preserves_metadata_and_body(db_session: AsyncSession) -> None:
    owner = await _user(db_session)
    raw = """---
name: Code Reviewer
description: Reviews code changes
version: 1.2.3
author: Example Team
license: MIT
icon: magnifying-glass
activation: Use when reviewing diffs
tools:
  - read
  - grep
capabilities:
  checks: style
ignored: not preserved
---
# Instructions

Review carefully.
"""

    skill = await skill_service.import_skill_from_md(db_session, raw, owner)

    assert skill.name == "Code Reviewer"
    assert skill.description == "Reviews code changes"
    assert skill.body_markdown == "# Instructions\n\nReview carefully.\n"
    assert skill.metadata_ is not None
    assert skill.metadata_["version"] == "1.2.3"
    assert skill.metadata_["author"] == "Example Team"
    assert skill.metadata_["tools"] == ["read", "grep"]
    assert skill.metadata_["ignored"] == "not preserved"


def _zip_bytes(entries: dict[str, bytes]) -> bytes:
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as zf:
        for path, payload in entries.items():
            zf.writestr(path, payload)
    return buffer.getvalue()


async def test_import_skill_zip_classifies_entries(db_session: AsyncSession) -> None:
    owner = await _user(db_session)
    payload = _zip_bytes(
        {
            "bundle/SKILL.md": b"---\nname: Packaged\ndescription: Demo\n---\nBody\n",
            "bundle/references/guide.md": b"guide",
            "bundle/scripts/setup.sh": b"script",
            "bundle/assets/icon.png": b"png",
            "bundle/tools/tool.json": b"{}",
            "bundle/misc.txt": b"misc",
        }
    )

    skill = await skill_service.import_skill_from_zip(db_session, payload, owner)

    assert skill.files is not None
    categories = {item["path"]: item["category"] for item in skill.files}
    assert categories == {
        "SKILL.md": "SKILL.md",
        "references/guide.md": "references",
        "scripts/setup.sh": "scripts",
        "assets/icon.png": "assets",
        "tools/tool.json": "tools",
        "misc.txt": "other",
    }


@pytest.mark.parametrize(
    "unsafe_path",
    ["../SKILL.md", "/tmp/SKILL.md", "C:/tmp/SKILL.md", "bundle/../SKILL.md"],
)
async def test_import_skill_zip_rejects_unsafe_paths(
    db_session: AsyncSession,
    unsafe_path: str,
) -> None:
    owner = await _user(db_session)
    payload = _zip_bytes(
        {
            "SKILL.md": b"---\nname: Safe\ndescription: Demo\n---\nBody\n",
            unsafe_path: b"bad",
        }
    )

    with pytest.raises(AgentChatError, match="unsafe path"):
        await skill_service.import_skill_from_zip(db_session, payload, owner)


async def test_import_skill_zip_stores_under_skillshub_when_root_set(
    db_session: AsyncSession, tmp_path: Path
) -> None:
    owner = await _user(db_session)
    db_session.add(
        SystemSettings(
            owner_id=owner.id,
            group_workspace_root=str(tmp_path.resolve()),
        )
    )
    await db_session.flush()

    payload = _zip_bytes(
        {
            "SKILL.md": b"---\nname: Hub Skill\ndescription: Demo\n---\nBody\n",
            "references/guide.md": b"guide",
        }
    )

    skill = await skill_service.import_skill_from_zip(db_session, payload, owner)

    expected_dir = tmp_path / "skillshub" / str(skill.id)
    assert expected_dir.is_dir()
    assert skill.storage_path == str(expected_dir.resolve())
    assert (expected_dir / "SKILL.md").is_file()
    assert (expected_dir / "references" / "guide.md").is_file()
