"""Sync ORM-added columns: groups extras, skills extras, group_files table.

Revision ID: 0004
Revises: 0003
Create Date: 2026-05-03

"""
from collections.abc import Sequence

import sqlalchemy as sa
from sqlalchemy.dialects import postgresql

from alembic import op

revision: str = "0004"
down_revision: str | None = "0003"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    # groups: 4 new columns
    op.add_column(
        "groups",
        sa.Column(
            "free_speech",
            sa.Boolean(),
            nullable=False,
            server_default=sa.false(),
        ),
    )
    op.add_column(
        "groups",
        sa.Column(
            "allow_agent_free_mention",
            sa.Boolean(),
            nullable=False,
            server_default=sa.true(),
        ),
    )
    op.add_column(
        "groups",
        sa.Column("muted_agent_ids", postgresql.JSONB(), nullable=True),
    )
    op.add_column(
        "groups",
        sa.Column("admin_agent_ids", postgresql.JSONB(), nullable=True),
    )

    # skills: 2 new columns
    op.add_column("skills", sa.Column("files", postgresql.JSONB(), nullable=True))
    op.add_column("skills", sa.Column("storage_path", sa.Text(), nullable=True))

    # group_files: new table
    op.create_table(
        "group_files",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True),
        sa.Column("group_id", postgresql.UUID(as_uuid=True), nullable=False),
        sa.Column("uploader_id", postgresql.UUID(as_uuid=True), nullable=False),
        sa.Column("filename", sa.String(255), nullable=False),
        sa.Column("file_path", sa.Text(), nullable=False),
        sa.Column(
            "file_size",
            sa.BigInteger(),
            nullable=False,
            server_default=sa.text("0"),
        ),
        sa.Column("mime_type", sa.String(100), nullable=True),
        sa.Column(
            "status",
            sa.String(30),
            nullable=False,
            server_default="active",
        ),
        sa.Column(
            "created_at",
            sa.DateTime(timezone=True),
            server_default=sa.func.now(),
            nullable=False,
        ),
    )
    op.create_index("ix_group_files_group_id", "group_files", ["group_id"])


def downgrade() -> None:
    op.drop_index("ix_group_files_group_id", table_name="group_files")
    op.drop_table("group_files")
    op.drop_column("skills", "storage_path")
    op.drop_column("skills", "files")
    op.drop_column("groups", "admin_agent_ids")
    op.drop_column("groups", "muted_agent_ids")
    op.drop_column("groups", "allow_agent_free_mention")
    op.drop_column("groups", "free_speech")
