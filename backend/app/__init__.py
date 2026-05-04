"""Backend application package."""

import asyncio
import sys

# psycopg async connections used by LangGraph's AsyncPostgresSaver are not
# compatible with Windows' default ProactorEventLoop. Set the Selector policy as
# soon as the app package is imported, before app startup opens psycopg
# connections. Non-Windows platforms keep their default policy.
if sys.platform == "win32":
    asyncio.set_event_loop_policy(asyncio.WindowsSelectorEventLoopPolicy())
