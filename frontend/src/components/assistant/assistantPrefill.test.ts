import { describe, expect, it } from 'vitest'

import { assistantPrefillLink, parseMcpPrefill, parseProviderPrefill } from './assistantPrefill'

const search = (value: unknown) => `?${new URLSearchParams({ assistant_prefill: JSON.stringify(value) })}`

describe('Assistant prefill parameters', () => {
  it.each(['', '?unrelated=value', '?assistant_prefill=%7Bbroken', search(null), search([]), search('text')])(
    'ignores missing or malformed parameters: %s', (value) => {
      expect(parseProviderPrefill(value)).toBeUndefined()
      expect(parseMcpPrefill(value)).toBeUndefined()
    },
  )

  it('bounds payloads and validates provider values without carrying credentials', () => {
    expect(parseProviderPrefill(search({ name: 'a'.repeat(8_193) }))).toBeUndefined()
    const result = parseProviderPrefill(search({
      name: 1, kind: 'unknown', default_model: {},
      base_url: 'https://user:password@example.com/v1', api_key: 'secret', headers: { Authorization: 'secret' },
    }))
    expect(JSON.stringify(result)).toBe('{}')
    expect(JSON.stringify(parseProviderPrefill(search({ base_url: 'https://example.com?token=secret' })))).toBe('{}')
  })

  it('only prefills HTTP/SSE MCP and validates optional field types', () => {
    expect(parseMcpPrefill(search({ transport: 'stdio', command: 'run' }))).toBeUndefined()
    expect(parseMcpPrefill(search({ name: 'missing transport' }))).toBeUndefined()
    const result = parseMcpPrefill(search({
      transport: 'http', timeout_seconds: 601, tool_filter: ['ok', 1],
      url: 'javascript:alert(1)', command: 'run', env: { KEY: 'secret' }, headers: { Authorization: 'secret' },
    }))
    expect(JSON.stringify(result)).toBe('{"transport":"streamable-http"}')
    expect(parseMcpPrefill(search({ transport: 'sse', timeout_seconds: 30, tool_filter: ['weather'] })))
      .toMatchObject({ transport: 'sse', timeout_seconds: 30, tool_filter: ['weather'] })
  })

  it('only encodes whitelisted fields for the two supported creation routes', () => {
    const result = assistantPrefillLink('/providers/new', { name: 'Primary', api_key: 'secret' })
    expect(parseProviderPrefill(result.route.slice(result.route.indexOf('?')))).toMatchObject({ name: 'Primary' })
    expect(result.route).not.toContain('secret')
    expect(assistantPrefillLink('/providers/id', { name: 'Edit' })).toEqual({ route: '/providers/id', fields: {}, prefilled: false })
    expect(assistantPrefillLink('/mcp-servers/new', { transport: 'stdio', command: 'run' }).prefilled).toBe(false)
  })
})
