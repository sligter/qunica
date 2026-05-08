"""Add Tavily WebSearch settings.

Revision ID: 0013
Revises: 0012
Create Date: 2026-05-08

"""
from collections.abc import Sequence

import sqlalchemy as sa

from alembic import op

revision: str = "0013"
down_revision: str | None = "0012"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.add_column(
        "system_settings",
        sa.Column("web_search_provider", sa.String(length=30), nullable=False, server_default="tavily"),
    )
    op.add_column("system_settings", sa.Column("tavily_api_key", sa.Text(), nullable=True))
    op.add_column(
        "system_settings",
        sa.Column(
            "tavily_search_url",
            sa.Text(),
            nullable=False,
            server_default="https://api.tavily.com/search",
        ),
    )
    op.add_column(
        "system_settings",
        sa.Column("tavily_max_results", sa.Integer(), nullable=False, server_default="5"),
    )
    op.add_column(
        "system_settings",
        sa.Column("tavily_search_depth", sa.String(length=20), nullable=False, server_default="basic"),
    )
    op.add_column(
        "system_settings",
        sa.Column("tavily_include_answer", sa.Boolean(), nullable=False, server_default="true"),
    )
    op.add_column(
        "system_settings",
        sa.Column("tavily_include_raw_content", sa.Boolean(), nullable=False, server_default="false"),
    )


def downgrade() -> None:
    op.drop_column("system_settings", "tavily_include_raw_content")
    op.drop_column("system_settings", "tavily_include_answer")
    op.drop_column("system_settings", "tavily_search_depth")
    op.drop_column("system_settings", "tavily_max_results")
    op.drop_column("system_settings", "tavily_search_url")
    op.drop_column("system_settings", "tavily_api_key")
    op.drop_column("system_settings", "web_search_provider")
