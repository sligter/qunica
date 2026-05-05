"""switch messages.created_at default from now() to clock_timestamp()

Revision ID: 0009
Revises: 0008
Create Date: 2026-05-05

PostgreSQL's `now()` returns the **transaction start time**, which means
all messages persisted within a single `send_message_stream` request
transaction (user message + agent reply) get identical `created_at`
values. `clock_timestamp()` returns the **statement time** instead, so
each INSERT inside the same transaction gets its own distinct
microsecond-precision value. This guarantees `ORDER BY created_at ASC`
returns the user message before the agent reply it triggered.

This is a metadata-only change (column DEFAULT clause); it does not
rewrite the table or backfill existing rows. Legacy rows keep their
colliding timestamps and are ordered deterministically by the
`Message.id` tie-breaker added to the relevant queries in the same PR.
"""
from collections.abc import Sequence

import sqlalchemy as sa

from alembic import op

revision: str = "0009"
down_revision: str | None = "0008"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.execute(
        sa.text(
            "ALTER TABLE messages ALTER COLUMN created_at "
            "SET DEFAULT clock_timestamp()"
        )
    )


def downgrade() -> None:
    op.execute(
        sa.text(
            "ALTER TABLE messages ALTER COLUMN created_at SET DEFAULT now()"
        )
    )
