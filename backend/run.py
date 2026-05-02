"""Launcher that forces SelectorEventLoop on Windows.

psycopg's async driver (used by langgraph PostgresSaver) requires a
SelectorEventLoop on Windows. uvicorn's `asyncio_loop_factory` hardcodes
ProactorEventLoop on Windows and ignores the asyncio event-loop policy, so
we bypass uvicorn's CLI loop creation by running the Server directly via
`asyncio.run(loop_factory=...)` (Python 3.12+).

Run with: `uv run python run.py`
"""

from __future__ import annotations

import asyncio
import sys

from uvicorn import Config, Server


def main() -> None:
    config = Config(
        "app.main:app",
        host="127.0.0.1",
        port=8000,
        log_level="info",
        lifespan="on",
    )
    server = Server(config)

    if sys.platform == "win32":
        asyncio.run(server.serve(), loop_factory=asyncio.SelectorEventLoop)
    else:
        asyncio.run(server.serve())


if __name__ == "__main__":
    main()
