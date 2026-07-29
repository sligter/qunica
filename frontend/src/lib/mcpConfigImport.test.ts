import { describe, expect, it } from 'vitest'

import { parseMcpConfig } from './mcpConfigImport'

describe('parseMcpConfig', () => {
  it('parses a stdio MCP configuration', () => {
    expect(parseMcpConfig(`{
      "args": ["chrome-devtools-mcp@latest"],
      "command": "npx",
      "type": "stdio"
    }`)).toMatchObject({
      transport: 'stdio',
      command: 'npx',
      args: ['chrome-devtools-mcp@latest'],
    })
  })

  it('normalizes an HTTP configuration', () => {
    expect(parseMcpConfig(`{
      "type": "http",
      "url": "https://example.com/mcp",
      "headers": { "Authorization": "Bearer token" }
    }`)).toMatchObject({
      transport: 'streamable-http',
      url: 'https://example.com/mcp',
      headers: { Authorization: 'Bearer token' },
    })
  })
})
