"""Add system_settings table for per-user global preferences.

Revision ID: 0008
Revises: 0007
Create Date: 2026-05-04

"""
from collections.abc import Sequence

import sqlalchemy as sa
from sqlalchemy.dialects import postgresql

from alembic import op

revision: str = "0008"
down_revision: str | None = "0007"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.create_table(
        "system_settings",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True),
        sa.Column("owner_id", postgresql.UUID(as_uuid=True), nullable=False, unique=True),
        sa.Column("group_workspace_root", sa.Text(), nullable=True),
        sa.Column(
            "created_at",
            sa.DateTime(timezone=True),
            server_default=sa.text("now()"),
            nullable=False,
        ),
        sa.Column(
            "updated_at",
            sa.DateTime(timezone=True),
            server_default=sa.text("now()"),
            nullable=False,
        ),
    )
    op.create_index("ix_system_settings_owner_id", "system_settings", ["owner_id"], unique=True)


def downgrade() -> None:
    op.drop_index("ix_system_settings_owner_id", table_name="system_settings")
    op.drop_table("system_settings")
