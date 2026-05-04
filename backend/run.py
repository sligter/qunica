"""Backward-compatible backend launcher.

Prefer the module form so arguments can be passed consistently:
    uv --directory backend run python -m app.dev_server --host 127.0.0.1 --port 8000
"""

from app.dev_server import main

if __name__ == "__main__":
    main()
