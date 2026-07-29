import type { McpTransport } from '@/types/api'

export interface ImportedMcpConfig {
  name?: string
  description?: string
  transport: McpTransport
  command?: string
  args: string[]
  env: Record<string, string>
  cwd?: string
  url?: string
  headers: Record<string, string>
  timeoutSeconds?: number
  toolFilter?: string[]
  enabled?: boolean
}

function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('Expected a JSON object')
  }
  return value as Record<string, unknown>
}

function optionalString(value: unknown): string | undefined {
  if (value === undefined || value === null) return undefined
  if (typeof value !== 'string') throw new Error('Expected a string')
  return value
}

function stringArray(value: unknown): string[] {
  if (value === undefined) return []
  if (!Array.isArray(value) || value.some((item) => typeof item !== 'string')) {
    throw new Error('Expected a string array')
  }
  return value
}

function stringRecord(value: unknown): Record<string, string> {
  if (value === undefined) return {}
  const parsed = record(value)
  if (Object.values(parsed).some((item) => typeof item !== 'string')) {
    throw new Error('Expected string values')
  }
  return parsed as Record<string, string>
}

export function parseMcpConfig(raw: string): ImportedMcpConfig {
  const config = record(JSON.parse(raw) as unknown)
  const rawTransport = config.type ?? config.transport ?? (config.url ? 'streamable-http' : 'stdio')
  const transport =
    rawTransport === 'http' ? 'streamable-http' : rawTransport
  if (transport !== 'stdio' && transport !== 'streamable-http' && transport !== 'sse') {
    throw new Error('Unsupported transport')
  }

  const timeoutSeconds = config.timeout_seconds ?? config.timeoutSeconds
  if (timeoutSeconds !== undefined && (typeof timeoutSeconds !== 'number' || timeoutSeconds < 1)) {
    throw new Error('Invalid timeout')
  }

  const enabled = config.enabled
  if (enabled !== undefined && typeof enabled !== 'boolean') {
    throw new Error('Invalid enabled value')
  }

  return {
    name: optionalString(config.name),
    description: optionalString(config.description),
    transport,
    command: optionalString(config.command),
    args: stringArray(config.args),
    env: stringRecord(config.env),
    cwd: optionalString(config.cwd),
    url: optionalString(config.url),
    headers: stringRecord(config.headers),
    timeoutSeconds,
    toolFilter: config.tool_filter === undefined
      ? undefined
      : stringArray(config.tool_filter),
    enabled,
  }
}
