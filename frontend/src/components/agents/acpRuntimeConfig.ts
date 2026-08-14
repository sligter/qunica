export const DEFAULT_ACP_TIMEOUT_SECONDS = 21_600

export function formatAcpArgs(args: string[] | undefined): string {
  return args?.join(' ') ?? ''
}

export function parseAcpArgs(value: string): string[] {
  return value.trim() ? value.trim().split(/\s+/) : []
}

export function formatAcpEnv(env: Record<string, string> | undefined): string {
  return Object.entries(env ?? {})
    .map(([key, value]) => `${key}=${value}`)
    .join('\n')
}

export function parseAcpEnv(value: string): Record<string, string> {
  return Object.fromEntries(
    value
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter(Boolean)
      .map((line) => {
        const index = line.indexOf('=')
        return index === -1
          ? [line, '']
          : [line.slice(0, index).trim(), line.slice(index + 1)]
      })
      .filter(([key]) => key.length > 0),
  )
}
