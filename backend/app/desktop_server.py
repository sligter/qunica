"""Desktop sidecar entrypoint.

PyInstaller packages this module as `ag-swarmer-backend.exe`. It configures
the backend for app-local SQLite before importing the FastAPI application.
"""

from __future__ import annotations

import argparse
import asyncio
import os
import secrets
import sys
from datetime import UTC, datetime
from pathlib import Path

from uvicorn import Config, Server


def _default_app_data_dir() -> Path:
    if os.environ.get("AG_SWARMER_APP_DATA"):
        return Path(os.environ["AG_SWARMER_APP_DATA"]).expanduser()
    base = os.environ.get("LOCALAPPDATA")
    if base:
        return Path(base) / "ag-swarmer"
    return Path.home() / ".ag-swarmer"


def _sqlite_url(path: Path) -> str:
    return f"sqlite+aiosqlite:///{path.resolve().as_posix()}"


def _desktop_secret_key(app_data_dir: Path) -> str:
    secret_file = app_data_dir / "desktop-secret.key"
    try:
        existing = secret_file.read_text(encoding="utf-8").strip()
    except FileNotFoundError:
        existing = ""
    if len(existing.encode("utf-8")) >= 32:
        return existing

    secret = secrets.token_urlsafe(48)
    secret_file.write_text(secret, encoding="utf-8")
    return secret


def _configure_env(app_data_dir: Path, port: int) -> None:
    app_data_dir.mkdir(parents=True, exist_ok=True)
    os.environ.setdefault("DESKTOP_APP_DATA_DIR", str(app_data_dir))
    os.environ.setdefault("SECRET_KEY", _desktop_secret_key(app_data_dir))
    os.environ.setdefault("DATABASE_URL", _sqlite_url(app_data_dir / "ag-swarmer.sqlite3"))
    os.environ.setdefault(
        "CHECKPOINT_DATABASE_URL",
        _sqlite_url(app_data_dir / "langgraph-checkpoints.sqlite3"),
    )
    os.environ.setdefault(
        "CORS_ORIGINS",
        '["http://tauri.localhost","https://tauri.localhost","http://localhost:5173"]',
    )
    os.environ.setdefault("DEBUG", "false")
    os.environ["AG_SWARMER_DESKTOP_PORT"] = str(port)


def _redirect_windowed_output(app_data_dir: Path) -> None:
    log_dir = app_data_dir / "logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    log_file_path = log_dir / "backend.log"
    log_file = log_file_path.open("a", encoding="utf-8", buffering=1)
    sys.stdout = log_file
    sys.stderr = log_file
    timestamp = datetime.now(UTC).isoformat()
    print(f"[{timestamp}] backend log started: {log_file_path}", flush=True)


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run the AgentChat desktop backend.")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", default=8765, type=int)
    parser.add_argument("--app-data-dir", default=None)
    parser.add_argument("--log-level", default="info")
    return parser.parse_args()


async def _serve(args: argparse.Namespace) -> None:
    app_data_dir = (
        Path(args.app_data_dir).expanduser()
        if args.app_data_dir
        else _default_app_data_dir()
    )
    _configure_env(app_data_dir, args.port)
    _redirect_windowed_output(app_data_dir)
    config = Config(
        "app.main:app",
        host=args.host,
        port=args.port,
        log_level=args.log_level,
        lifespan="on",
    )
    await Server(config).serve()


def main() -> None:
    args = _parse_args()
    if sys.platform == "win32":
        asyncio.run(_serve(args), loop_factory=asyncio.SelectorEventLoop)
    else:
        asyncio.run(_serve(args))


if __name__ == "__main__":
    main()
