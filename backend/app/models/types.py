from __future__ import annotations

from typing import Any, cast
from uuid import UUID

from sqlalchemy import CHAR, JSON
from sqlalchemy.dialects.postgresql import JSONB
from sqlalchemy.dialects.postgresql import UUID as PgUUID
from sqlalchemy.engine.interfaces import Dialect
from sqlalchemy.types import TypeDecorator, TypeEngine


class GUID(TypeDecorator[UUID]):
    """Portable UUID column.

    PostgreSQL keeps native UUID values; SQLite stores canonical string UUIDs.
    """

    impl = CHAR
    cache_ok = True

    def load_dialect_impl(self, dialect: Dialect) -> TypeEngine[Any]:
        if dialect.name == "postgresql":
            return dialect.type_descriptor(cast(TypeEngine[Any], PgUUID(as_uuid=True)))
        return dialect.type_descriptor(cast(TypeEngine[Any], CHAR(36)))

    def process_bind_param(
        self, value: UUID | str | None, dialect: Dialect
    ) -> UUID | str | None:
        if value is None:
            return None
        if dialect.name == "postgresql":
            return value if isinstance(value, UUID) else UUID(str(value))
        return str(value if isinstance(value, UUID) else UUID(str(value)))

    def process_result_value(self, value: UUID | str | None, dialect: Dialect) -> UUID | None:
        _ = dialect
        if value is None:
            return None
        return value if isinstance(value, UUID) else UUID(str(value))


JSONData = JSON().with_variant(JSONB(), "postgresql")
