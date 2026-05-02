from collections.abc import AsyncIterator
from contextlib import asynccontextmanager

from fastapi import FastAPI, Request
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse
from langgraph.checkpoint.postgres.aio import AsyncPostgresSaver

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


def _psycopg_url(asyncpg_url: str) -> str:
    """LangGraph PostgresSaver expects a psycopg-style URL, not the SQLAlchemy
    asyncpg dialect URL we use for SQLAlchemy."""
    return asyncpg_url.replace("postgresql+asyncpg://", "postgresql://")


@asynccontextmanager
async def lifespan(app: FastAPI) -> AsyncIterator[None]:
    setup_logging()
    async with AsyncPostgresSaver.from_conn_string(
        _psycopg_url(settings.database_url)
    ) as checkpointer:
        await checkpointer.setup()
        app.state.graph = compile_graph(checkpointer)
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
