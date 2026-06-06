"""add group agent free mention follow-up limit

Revision ID: 0017
Revises: 0016
Create Date: 2026-06-03
"""

from collections.abc import Sequence

import sqlalchemy as sa

from alembic import op

revision: str = "0017"
down_revision: str | None = "0016"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.add_column(
        "groups",
        sa.Column(
            "agent_free_mention_max_dispatches",
            sa.Integer(),
            server_default=sa.text("8"),
            nullable=False,
        ),
    )
    op.create_check_constraint(
        "ck_groups_agent_free_mention_max_dispatches_min",
        "groups",
        "agent_free_mention_max_dispatches >= 0",
    )


def downgrade() -> None:
    op.drop_constraint(
        "ck_groups_agent_free_mention_max_dispatches_min",
        "groups",
        type_="check",
    )
    op.drop_column("groups", "agent_free_mention_max_dispatches")
