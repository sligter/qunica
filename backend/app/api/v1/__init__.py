from fastapi import APIRouter

from app.api.v1 import (
    agents,
    auth,
    groups,
    health,
    llm_providers,
    skills,
    threads,
    workspaces,
)

api_router = APIRouter()
api_router.include_router(health.router, tags=["health"])
api_router.include_router(auth.router)
api_router.include_router(agents.router)
api_router.include_router(groups.router)
api_router.include_router(threads.router)
api_router.include_router(llm_providers.router)
api_router.include_router(skills.router)
api_router.include_router(workspaces.router)
