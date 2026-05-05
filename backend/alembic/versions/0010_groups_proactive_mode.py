"""add proactive group participation settings

Revision ID: 0010
Revises: 0009
Create Date: 2026-05-05
"""

from collections.abc import Sequence

import sqlalchemy as sa

from alembic import op

revision: str = "0010"
down_revision: str | None = "0009"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.add_column(
        "groups",
        sa.Column(
            "proactive_mode",
            sa.Boolean(),
            server_default=sa.false(),
            nullable=False,
        ),
    )
    op.add_column(
        "groups",
        sa.Column(
            "proactive_max_rounds",
            sa.Integer(),
            server_default=sa.text("1"),
            nullable=False,
        ),
    )
    op.create_check_constraint(
        "ck_groups_proactive_max_rounds_range",
        "groups",
        "proactive_max_rounds BETWEEN 1 AND 5",
    )


def downgrade() -> None:
    op.drop_constraint(
        "ck_groups_proactive_max_rounds_range",
        "groups",
        type_="check",
    )
    op.drop_column("groups", "proactive_max_rounds")
    op.drop_column("groups", "proactive_mode")
