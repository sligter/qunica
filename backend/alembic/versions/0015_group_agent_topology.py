"""Add group agent topology fields.

Revision ID: 0015
Revises: 0014
Create Date: 2026-05-09

"""
from collections.abc import Sequence

import sqlalchemy as sa

from alembic import op

revision: str = "0015"
down_revision: str | None = "0014"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.add_column(
        "group_agents",
        sa.Column("topology_role", sa.String(length=30), nullable=True),
    )
    op.add_column(
        "group_agents",
        sa.Column("speaking_order", sa.Integer(), nullable=True),
    )
    op.create_check_constraint(
        "ck_group_agents_topology_role",
        "group_agents",
        "topology_role IS NULL OR topology_role IN ('hub', 'leader', 'worker')",
    )
    op.create_check_constraint(
        "ck_group_agents_speaking_order_min",
        "group_agents",
        "speaking_order IS NULL OR speaking_order >= 1",
    )
    op.create_index(
        "ix_group_agents_group_topology_role",
        "group_agents",
        ["group_id", "topology_role"],
    )
    op.create_index(
        "ix_group_agents_group_speaking_order",
        "group_agents",
        ["group_id", "speaking_order"],
    )
    op.execute(
        """
        UPDATE group_agents AS ga
        SET topology_role = CASE
            WHEN g.communication_mode = 'star' THEN 'hub'
            WHEN g.communication_mode = 'hierarchical' THEN 'leader'
            ELSE ga.topology_role
        END
        FROM groups AS g
        WHERE g.id = ga.group_id
          AND ga.status = 'active'
          AND g.admin_agent_ids IS NOT NULL
          AND g.admin_agent_ids ? ga.agent_id::text
          AND g.communication_mode IN ('star', 'hierarchical')
        """
    )
    op.execute(
        """
        WITH ordered AS (
            SELECT
                ga.id,
                row_number() OVER (
                    PARTITION BY ga.group_id
                    ORDER BY ga.joined_at ASC, ga.id ASC
                ) AS rn
            FROM group_agents AS ga
            JOIN groups AS g ON g.id = ga.group_id
            WHERE g.communication_mode = 'ring'
              AND ga.status = 'active'
        )
        UPDATE group_agents AS ga
        SET speaking_order = ordered.rn
        FROM ordered
        WHERE ordered.id = ga.id
        """
    )


def downgrade() -> None:
    op.drop_index("ix_group_agents_group_speaking_order", table_name="group_agents")
    op.drop_index("ix_group_agents_group_topology_role", table_name="group_agents")
    op.drop_constraint(
        "ck_group_agents_speaking_order_min", "group_agents", type_="check"
    )
    op.drop_constraint("ck_group_agents_topology_role", "group_agents", type_="check")
    op.drop_column("group_agents", "speaking_order")
    op.drop_column("group_agents", "topology_role")
