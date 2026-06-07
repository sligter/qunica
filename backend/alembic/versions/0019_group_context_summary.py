"""add group rolling context summary

Revision ID: 0019
Revises: 0018
Create Date: 2026-06-07
"""

from collections.abc import Sequence

import sqlalchemy as sa
from sqlalchemy.dialects import postgresql

from alembic import op

revision: str = "0019"
down_revision: str | None = "0018"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.add_column(
        "llm_providers",
        sa.Column("context_window_tokens", sa.Integer(), nullable=True),
    )
    op.add_column(
        "llm_providers",
        sa.Column("context_output_reserve_ratio", sa.Float(), nullable=True),
    )
    op.add_column("groups", sa.Column("context_summary", sa.Text(), nullable=True))
    op.add_column(
        "groups",
        sa.Column(
            "context_summary_message_id",
            postgresql.UUID(as_uuid=True),
            nullable=True,
        ),
    )
    op.add_column(
        "groups",
        sa.Column("context_summary_updated_at", sa.DateTime(timezone=True), nullable=True),
    )
    op.add_column(
        "group_agents",
        sa.Column("last_context_input_tokens", sa.Integer(), nullable=True),
    )
    op.add_column(
        "group_agents",
        sa.Column("last_context_output_tokens", sa.Integer(), nullable=True),
    )
    op.add_column(
        "group_agents",
        sa.Column("last_context_total_tokens", sa.Integer(), nullable=True),
    )
    op.add_column(
        "group_agents",
        sa.Column("last_context_window_tokens", sa.Integer(), nullable=True),
    )
    op.add_column(
        "group_agents",
        sa.Column("last_context_output_reserve_tokens", sa.Integer(), nullable=True),
    )
    op.add_column(
        "group_agents",
        sa.Column(
            "last_context_message_id",
            postgresql.UUID(as_uuid=True),
            nullable=True,
        ),
    )
    op.add_column(
        "group_agents",
        sa.Column("last_context_usage_source", sa.String(length=30), nullable=True),
    )
    op.add_column(
        "group_agents",
        sa.Column("last_context_updated_at", sa.DateTime(timezone=True), nullable=True),
    )


def downgrade() -> None:
    op.drop_column("group_agents", "last_context_updated_at")
    op.drop_column("group_agents", "last_context_usage_source")
    op.drop_column("group_agents", "last_context_message_id")
    op.drop_column("group_agents", "last_context_output_reserve_tokens")
    op.drop_column("group_agents", "last_context_window_tokens")
    op.drop_column("group_agents", "last_context_total_tokens")
    op.drop_column("group_agents", "last_context_output_tokens")
    op.drop_column("group_agents", "last_context_input_tokens")
    op.drop_column("groups", "context_summary_updated_at")
    op.drop_column("groups", "context_summary_message_id")
    op.drop_column("groups", "context_summary")
    op.drop_column("llm_providers", "context_output_reserve_ratio")
    op.drop_column("llm_providers", "context_window_tokens")
