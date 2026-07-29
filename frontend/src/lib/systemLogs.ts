export const LOG_LEVELS = [
  'off',
  'error',
  'warn',
  'info',
  'debug',
  'trace',
] as const

export type LogLevel = (typeof LOG_LEVELS)[number]

export interface ModuleLogOverride {
  target: string
  level: LogLevel
}

export interface LogFilterConfig {
  level: LogLevel
  overrides: ModuleLogOverride[]
}

export interface SystemLogEntry {
  timestamp: string
  level: string
  target: string
  message: string
  fields: Record<string, unknown>
}

export interface SystemLogSnapshot {
  filter: string
  log_dir: string
  entries: SystemLogEntry[]
}

function isLogLevel(value: string): value is LogLevel {
  return (LOG_LEVELS as readonly string[]).includes(value)
}

export function parseLogFilter(filter: string): LogFilterConfig {
  let level: LogLevel = 'info'
  const overrides: ModuleLogOverride[] = []

  for (const directive of filter.split(',').map((value) => value.trim())) {
    if (!directive) continue
    const separator = directive.lastIndexOf('=')
    if (separator === -1) {
      if (isLogLevel(directive)) level = directive
      continue
    }
    const target = directive.slice(0, separator).trim()
    const overrideLevel = directive.slice(separator + 1).trim()
    if (target && isLogLevel(overrideLevel)) {
      overrides.push({ target, level: overrideLevel })
    }
  }

  return { level, overrides }
}

export function formatLogFilter(config: LogFilterConfig): string {
  return [
    config.level,
    ...config.overrides
      .filter(({ target }) => target.trim())
      .map(({ target, level }) => `${target.trim()}=${level}`),
  ].join(',')
}
