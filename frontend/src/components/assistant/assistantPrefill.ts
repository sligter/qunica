import type { ProviderKind } from '@/types/api'

export interface ProviderPrefill {
  name?: string
  kind?: ProviderKind
  base_url?: string
  default_model?: string
  description?: string
}

export interface McpPrefill {
  name?: string
  description?: string
  transport: 'streamable-http' | 'sse'
  url?: string
  timeout_seconds?: number
  tool_filter?: string[]
}

const PARAM = 'assistant_prefill'
const MAX_PARAM_LENGTH = 8_192
const PROVIDER_KINDS: ProviderKind[] = [
  'openai-compatible', 'openai-responses', 'anthropic', 'anthropic-compatible', 'gemini',
]

function record(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null
}

function text(value: unknown, maximum = 2_000): string | undefined {
  return typeof value === 'string' && value.trim().length > 0 && value.length <= maximum
    ? value.trim()
    : undefined
}

function endpoint(value: unknown): string | undefined {
  const raw = text(value)
  if (!raw) return undefined
  try {
    const url = new URL(raw)
    // Note: URLs travel in navigation history; credential-bearing URL parts
    // must stay in the manually completed form, alongside keys and headers.
    return ['http:', 'https:'].includes(url.protocol)
      && !url.username && !url.password && !url.search && !url.hash ? raw : undefined
  } catch {
    return undefined
  }
}

function providerFields(value: unknown): ProviderPrefill | undefined {
  const fields = record(value)
  if (!fields) return undefined
  const kind = PROVIDER_KINDS.find((kind) => kind === fields.kind)
  return {
    name: text(fields.name, 100),
    kind,
    base_url: endpoint(fields.base_url),
    default_model: text(fields.default_model, 200),
    description: text(fields.description),
  }
}

function mcpFields(value: unknown): McpPrefill | undefined {
  const fields = record(value)
  if (!fields) return undefined
  const transport = fields.transport === 'http' || fields.transport === 'streamable-http'
    ? 'streamable-http' : fields.transport === 'sse' ? 'sse' : undefined
  if (!transport) return undefined
  return {
    name: text(fields.name, 100),
    description: text(fields.description),
    transport,
    url: endpoint(fields.url),
    timeout_seconds: typeof fields.timeout_seconds === 'number'
      && Number.isInteger(fields.timeout_seconds)
      && fields.timeout_seconds >= 1 && fields.timeout_seconds <= 600
      ? fields.timeout_seconds : undefined,
    tool_filter: Array.isArray(fields.tool_filter) && fields.tool_filter.length <= 100
      && fields.tool_filter.every((entry) => text(entry, 200) !== undefined)
      ? fields.tool_filter as string[] : undefined,
  }
}

function parse(search: string): unknown {
  const raw = new URLSearchParams(search).get(PARAM)
  if (!raw || raw.length > MAX_PARAM_LENGTH) return undefined
  try { return JSON.parse(raw) } catch { return undefined }
}

export function parseProviderPrefill(search: string): ProviderPrefill | undefined {
  return providerFields(parse(search))
}

export function parseMcpPrefill(search: string): McpPrefill | undefined {
  return mcpFields(parse(search))
}

export function assistantPrefillLink(route: string, fields: unknown): {
  route: string
  fields: Record<string, unknown>
  prefilled: boolean
} {
  // Note: both browser navigation and the existing desktop library command
  // carry the same URL. Only these creation forms accept a prefill payload.
  const safe = route === '/providers/new' ? providerFields(fields)
    : route === '/mcp-servers/new' ? mcpFields(fields) : undefined
  const entries = Object.entries(safe ?? {}).filter(([, value]) => value !== undefined)
  if (entries.length === 0) return { route, fields: {}, prefilled: false }
  const clean = Object.fromEntries(entries)
  const payload = JSON.stringify(clean)
  if (payload.length > MAX_PARAM_LENGTH) return { route, fields: {}, prefilled: false }
  const search = new URLSearchParams({ [PARAM]: payload })
  return { route: `${route}?${search}`, fields: clean, prefilled: true }
}
