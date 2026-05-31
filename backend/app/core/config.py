from pathlib import Path
from typing import Any

from pydantic import field_validator
from pydantic_settings import BaseSettings, SettingsConfigDict

PROJECT_ROOT = Path(__file__).resolve().parent.parent.parent


class Settings(BaseSettings):
    model_config = SettingsConfigDict(
        env_file=PROJECT_ROOT / ".env",
        env_file_encoding="utf-8",
        case_sensitive=False,
        extra="ignore",
    )

    app_name: str = "AgentChat"
    debug: bool = False

    # Auth / JWT
    secret_key: str = "please-change-me-in-production"
    algorithm: str = "HS256"
    access_token_expire_minutes: int = 60 * 24 * 7  # 7 days

    # Infrastructure
    database_url: str = (
        "postgresql+asyncpg://agentchat:agentchat@localhost:5432/agentchat"
    )
    checkpoint_database_url: str = ""
    desktop_app_data_dir: str = ""
    redis_url: str = "redis://localhost:6379/0"

    minio_endpoint: str = "localhost:9000"
    minio_access_key: str = "minioadmin"
    minio_secret_key: str = "minioadmin"
    minio_bucket: str = "agentchat"

    cors_origins: list[str] = ["http://localhost:5173"]

    # LLM defaults — agent-level llm_config can override per call
    llm_base_url: str = "https://api.openai.com/v1"
    llm_api_key: str = ""
    llm_default_model: str = "gpt-4o-mini"

    # Optional runtime tool providers. Tools remain bound without these values and
    # return controlled setup-required results when invoked.
    tavily_api_key: str = ""
    tavily_search_url: str = "https://api.tavily.com/search"
    playwright_search_url: str = ""

    @field_validator("debug", mode="before")
    @classmethod
    def _parse_debug(cls, value: object) -> object:
        if isinstance(value, str) and value.strip().lower() in {"release", "prod", "production"}:
            return False
        return value

    @property
    def effective_checkpoint_database_url(self) -> str:
        return self.checkpoint_database_url or self.database_url

    @property
    def is_sqlite(self) -> bool:
        return self.database_url.startswith("sqlite")

    def sqlite_connect_args(self) -> dict[str, Any]:
        if self.database_url.startswith("sqlite"):
            return {"check_same_thread": False}
        return {}


settings = Settings()
