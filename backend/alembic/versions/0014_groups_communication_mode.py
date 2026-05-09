"""Add group communication mode.

Revision ID: 0014
Revises: 0013
Create Date: 2026-05-09

"""
from collections.abc import Sequence

import sqlalchemy as sa

from alembic import op

revision: str = "0014"
down_revision: str | None = "0013"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.add_column(
        "groups",
        sa.Column(
            "communication_mode",
            sa.String(length=30),
            nullable=False,
            server_default="mesh",
        ),
    )
    op.create_check_constraint(
        "ck_groups_communication_mode",
        "groups",
        "communication_mode IN ('mesh', 'star', 'hierarchical', 'ring')",
    )


def downgrade() -> None:
    op.drop_constraint("ck_groups_communication_mode", "groups", type_="check")
    op.drop_column("groups", "communication_mode")
