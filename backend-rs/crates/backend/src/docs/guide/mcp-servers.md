# MCP servers

Registering a Model Context Protocol server lets agents call its tools as if they were built in.

## Transports

| Transport | How it works | Needs |
| --- | --- | --- |
| `stdio` | Launches a local process and speaks line-delimited JSON-RPC over its stdin/stdout | `command`, optional `args`, `env`, `cwd` |
| `http` | One HTTP endpoint taking JSON-RPC POSTs; sessions held by `Mcp-Session-Id` | `url`, optional `headers` |
| `sse` | Legacy: a GET event stream plus a POST message endpoint | `url`, optional `headers` |

**A `stdio` server launches a program on this machine with the environment you give it.** Only register commands you trust.

## Tool naming

Tools are exposed as `mcp__<server-slug>__<tool-name>`, so two servers offering the same tool name do not collide. If two server names slugify identically, the collision is refused and reported rather than silently routing to the wrong one.

## Narrowing what is offered

- A **tool filter** on the server limits which of its tools are exposed at all.
- Each agent then picks a subset of those in its own tool settings.

## Testing

**Test connection** dials the server and lists what it actually exposes. Run it before saving.

## Failure behavior

A server that cannot be reached costs only its own tools for that turn. The system prompt notes why it contributed nothing, and the turn proceeds. Listing runs concurrently under a shared deadline, so one hanging server cannot stall the turn.

## Secrets

Header values are masked whenever the API returns them, and updates that send a masked value keep the stored one. `stdio` servers inherit the app's environment with the configured overrides applied on top.
