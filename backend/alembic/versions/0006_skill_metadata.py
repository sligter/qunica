"""Add skill metadata and group muted member tracking.

Revision ID: 0006
Revises: 0005
Create Date: 2026-05-04

"""
from collections.abc import Sequence

import sqlalchemy as sa
from sqlalchemy.dialects import postgresql

from alembic import op

revision: str = "0006"
down_revision: str | None = "0005"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.add_column("skills", sa.Column("metadata", postgresql.JSONB(), nullable=True))
    op.add_column("groups", sa.Column("muted_member_ids", postgresql.JSONB(), nullable=True))


def downgrade() -> None:
    op.drop_column("groups", "muted_member_ids")
    op.drop_column("skills", "metadata")
