"""add proactive reply multiplier

Revision ID: 0011
Revises: 0010
Create Date: 2026-05-05
"""

from collections.abc import Sequence

import sqlalchemy as sa

from alembic import op

revision: str = "0011"
down_revision: str | None = "0010"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.add_column(
        "groups",
        sa.Column(
            "proactive_reply_multiplier",
            sa.Integer(),
            server_default=sa.text("1"),
            nullable=False,
        ),
    )
    op.create_check_constraint(
        "ck_groups_proactive_reply_multiplier_min",
        "groups",
        "proactive_reply_multiplier >= 1",
    )


def downgrade() -> None:
    op.drop_constraint(
        "ck_groups_proactive_reply_multiplier_min",
        "groups",
        type_="check",
    )
    op.drop_column("groups", "proactive_reply_multiplier")
