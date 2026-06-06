from collections.abc import AsyncIterator
from contextlib import asynccontextmanager
from pathlib import Path

from fastapi import FastAPI, Request
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse
from langgraph.checkpoint.postgres.aio import AsyncPostgresSaver
from sqlalchemy.engine import make_url
from sqlalchemy.ext.asyncio import AsyncConnection

from app.agents.runtime import compile_graph
from app.api.v1 import api_router
from app.core.config import settings
from app.core.exceptions import (
    AgentChatError,
    ConflictError,
    LLMProviderError,
    NotFoundError,
    PermissionDeniedError,
)
from app.core.logging import setup_logging
from app.db import engine
from app.models import Base


def _psycopg_url(asyncpg_url: str) -> str:
    """LangGraph PostgresSaver expects a psycopg-style URL, not the SQLAlchemy
    asyncpg dialect URL we use for SQLAlchemy."""
    return asyncpg_url.replace("postgresql+asyncpg://", "postgresql://")


def _sqlite_path(sqlite_url: str) -> str:
    database = make_url(sqlite_url).database
    if not database:
        raise RuntimeError("sqlite database URL must include a file path")
    path = Path(database)
    path.parent.mkdir(parents=True, exist_ok=True)
    return str(path)


async def _apply_sqlite_schema_patches(conn: AsyncConnection) -> None:
    result = await conn.exec_driver_sql("PRAGMA table_info(groups)")
    columns = {str(row[1]) for row in result.fetchall()}
    if "agent_free_mention_max_dispatches" not in columns:
        await conn.exec_driver_sql(
            """
            ALTER TABLE groups
            ADD COLUMN agent_free_mention_max_dispatches INTEGER NOT NULL DEFAULT 8
            CHECK (agent_free_mention_max_dispatches >= 0)
            """
        )


async def _bootstrap_sqlite_schema() -> None:
    async with engine.begin() as conn:
        await conn.run_sync(Base.metadata.create_all)
        await _apply_sqlite_schema_patches(conn)



@asynccontextmanager
async def lifespan(app: FastAPI) -> AsyncIterator[None]:
    setup_logging()
    if settings.is_sqlite:
        import aiosqlite
        from langgraph.checkpoint.sqlite.aio import AsyncSqliteSaver

        await _bootstrap_sqlite_schema()
        checkpoint_url = settings.effective_checkpoint_database_url
        async with aiosqlite.connect(_sqlite_path(checkpoint_url)) as conn:
            sqlite_checkpointer = AsyncSqliteSaver(conn)
            await sqlite_checkpointer.setup()
            app.state.graph = compile_graph(sqlite_checkpointer)
            yield
        return

    async with AsyncPostgresSaver.from_conn_string(
        _psycopg_url(settings.effective_checkpoint_database_url)
    ) as postgres_checkpointer:
        await postgres_checkpointer.setup()
        app.state.graph = compile_graph(postgres_checkpointer)
        yield


app = FastAPI(
    title="AgentChat",
    description="群式多 Agent 协作工作台",
    version="0.1.0",
    lifespan=lifespan,
)

app.add_middleware(
    CORSMiddleware,
    allow_origins=settings.cors_origins,
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


def _error(code: str, message: str, status_code: int) -> JSONResponse:
    return JSONResponse(
        status_code=status_code,
        content={"error": {"code": code, "message": message}},
    )


@app.exception_handler(NotFoundError)
async def _not_found(_: Request, exc: NotFoundError) -> JSONResponse:
    return _error("not_found", str(exc), 404)


@app.exception_handler(PermissionDeniedError)
async def _permission_denied(_: Request, exc: PermissionDeniedError) -> JSONResponse:
    return _error("permission_denied", str(exc), 403)


@app.exception_handler(ConflictError)
async def _conflict(_: Request, exc: ConflictError) -> JSONResponse:
    return _error("conflict", str(exc), 409)


@app.exception_handler(LLMProviderError)
async def _llm_error(_: Request, exc: LLMProviderError) -> JSONResponse:
    return _error("llm_provider_error", str(exc), 502)


@app.exception_handler(AgentChatError)
async def _domain_error(_: Request, exc: AgentChatError) -> JSONResponse:
    return _error("domain_error", str(exc), 400)


app.include_router(api_router, prefix="/api/v1")
