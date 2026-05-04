"""Development server launcher with a Windows-compatible event loop.

Run with:
    uv --directory backend run python -m app.dev_server --host 127.0.0.1 --port 8000
"""

from __future__ import annotations

import argparse
import asyncio
import sys

from uvicorn import Config, Server


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run the AgentChat backend dev server.")
    parser.add_argument("--host", default="127.0.0.1", help="Host interface to bind.")
    parser.add_argument("--port", default=8000, type=int, help="Port to bind.")
    parser.add_argument("--reload", action="store_true", help="Reload on source changes.")
    parser.add_argument("--log-level", default="info", help="Uvicorn log level.")
    return parser.parse_args()


async def _serve(args: argparse.Namespace) -> None:
    config = Config(
        "app.main:app",
        host=args.host,
        port=args.port,
        reload=args.reload,
        log_level=args.log_level,
        lifespan="on",
    )
    server = Server(config)
    await server.serve()


def main() -> None:
    args = _parse_args()

    if sys.platform == "win32":
        # psycopg's async driver is not compatible with Windows' default
        # ProactorEventLoop. Creating the loop explicitly is more reliable than
        # relying on global event-loop policy when uvicorn is started as a module.
        asyncio.run(_serve(args), loop_factory=asyncio.SelectorEventLoop)
    else:
        asyncio.run(_serve(args))


if __name__ == "__main__":
    main()
