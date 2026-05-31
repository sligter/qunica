"""Add external agent runtime fields and audit table.

Revision ID: 0016
Revises: 0015
Create Date: 2026-05-31

"""

from collections.abc import Sequence

import sqlalchemy as sa
from sqlalchemy.dialects import postgresql

from alembic import op

revision: str = "0016"
down_revision: str | None = "0015"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.add_column(
        "agents",
        sa.Column(
            "runtime_kind",
            sa.String(length=30),
            nullable=False,
            server_default="llm_chat",
        ),
    )
    op.add_column(
        "agents",
        sa.Column("external_runtime", postgresql.JSONB(), nullable=True),
    )
    op.create_table(
        "external_agent_runs",
        sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True),
        sa.Column("owner_id", postgresql.UUID(as_uuid=True), nullable=False),
        sa.Column("group_id", postgresql.UUID(as_uuid=True), nullable=True),
        sa.Column("agent_id", postgresql.UUID(as_uuid=True), nullable=False),
        sa.Column("thread_id", postgresql.UUID(as_uuid=True), nullable=True),
        sa.Column("adapter", sa.String(length=50), nullable=False),
        sa.Column("cwd", sa.Text(), nullable=False),
        sa.Column("status", sa.String(length=30), nullable=False, server_default="running"),
        sa.Column("argv", postgresql.JSONB(), nullable=False, server_default="[]"),
        sa.Column("exit_code", sa.Integer(), nullable=True),
        sa.Column("stdout_tail", sa.Text(), nullable=True),
        sa.Column("stderr_tail", sa.Text(), nullable=True),
        sa.Column("error_message", sa.Text(), nullable=True),
        sa.Column(
            "started_at",
            sa.DateTime(timezone=True),
            server_default=sa.func.now(),
            nullable=False,
        ),
        sa.Column("ended_at", sa.DateTime(timezone=True), nullable=True),
    )
    op.create_index("ix_external_agent_runs_owner_id", "external_agent_runs", ["owner_id"])
    op.create_index("ix_external_agent_runs_group_id", "external_agent_runs", ["group_id"])
    op.create_index("ix_external_agent_runs_agent_id", "external_agent_runs", ["agent_id"])
    op.create_index("ix_external_agent_runs_thread_id", "external_agent_runs", ["thread_id"])
    op.alter_column("agents", "runtime_kind", server_default=None)


def downgrade() -> None:
    op.drop_index("ix_external_agent_runs_thread_id", table_name="external_agent_runs")
    op.drop_index("ix_external_agent_runs_agent_id", table_name="external_agent_runs")
    op.drop_index("ix_external_agent_runs_group_id", table_name="external_agent_runs")
    op.drop_index("ix_external_agent_runs_owner_id", table_name="external_agent_runs")
    op.drop_table("external_agent_runs")
    op.drop_column("agents", "external_runtime")
    op.drop_column("agents", "runtime_kind")
