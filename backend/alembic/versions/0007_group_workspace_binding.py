"""Add workspace binding to groups.

Revision ID: 0007
Revises: 0006
Create Date: 2026-05-04

"""
from collections.abc import Sequence

import sqlalchemy as sa
from sqlalchemy.dialects import postgresql

from alembic import op

revision: str = "0007"
down_revision: str | None = "0006"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.add_column(
        "groups",
        sa.Column("workspace_id", postgresql.UUID(as_uuid=True), nullable=True),
    )
    op.create_index("ix_groups_workspace_id", "groups", ["workspace_id"])


def downgrade() -> None:
    op.drop_index("ix_groups_workspace_id", table_name="groups")
    op.drop_column("groups", "workspace_id")
