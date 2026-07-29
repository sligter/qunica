import { useEffect, useMemo, useRef, useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import {
  ChevronRight,
  FolderOpen,
  Pause,
  Play,
  Plus,
  RefreshCw,
  Search,
  Trash2,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { DetailShell } from '@/components/layout/DetailShell'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Section } from '@/components/ui/section'
import { SettingsRow, SettingsSection } from '@/components/ui/settings-row'
import {
  clearSystemLogs,
  getSystemLogs,
  isDesktopRuntime,
  openSystemLogsFolder,
  setSystemLogFilter,
} from '@/lib/desktop'
import {
  formatLogFilter,
  LOG_LEVELS,
  parseLogFilter,
  type LogFilterConfig,
  type LogLevel,
  type SystemLogEntry,
} from '@/lib/systemLogs'
import { cn } from '@/lib/utils'

type EntryLevelFilter = 'all' | Exclude<LogLevel, 'off'>

const ENTRY_LEVELS: EntryLevelFilter[] = [
  'all',
  'error',
  'warn',
  'info',
  'debug',
  'trace',
]

const LEVEL_CLASS: Record<string, string> = {
  error: 'text-destructive',
  warn: 'text-warning-foreground',
  info: 'text-primary',
  debug: 'text-success',
  trace: 'text-muted-foreground',
}

function withOverride(
  config: LogFilterConfig,
  index: number,
  patch: Partial<LogFilterConfig['overrides'][number]>,
): LogFilterConfig {
  return {
    ...config,
    overrides: config.overrides.map((override, overrideIndex) =>
      overrideIndex === index ? { ...override, ...patch } : override,
    ),
  }
}

function formatTimestamp(value: string, locale: string): string {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return new Intl.DateTimeFormat(locale, {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    fractionalSecondDigits: 3,
    hour12: false,
  }).format(date)
}

function LogEntryRow({ entry, locale }: { entry: SystemLogEntry; locale: string }) {
  const level = entry.level.toLowerCase()
  return (
    <details className="group border-b border-border last:border-b-0">
      <summary className="grid min-w-[52rem] cursor-pointer list-none grid-cols-[1rem_7.5rem_4.5rem_minmax(12rem,18rem)_1fr] items-start gap-3 px-3 py-2 text-xs marker:content-none hover:bg-card-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring">
        <ChevronRight className="mt-0.5 h-3.5 w-3.5 text-muted-foreground transition-transform group-open:rotate-90" />
        <time className="font-mono tabular-nums text-muted-foreground" dateTime={entry.timestamp}>
          {formatTimestamp(entry.timestamp, locale)}
        </time>
        <span className={cn('font-mono font-semibold uppercase', LEVEL_CLASS[level])}>
          {entry.level}
        </span>
        <span className="truncate font-mono text-muted-foreground" title={entry.target}>
          {entry.target}
        </span>
        <span className="break-words font-mono text-foreground">{entry.message}</span>
      </summary>
      <pre className="min-w-[52rem] overflow-x-auto border-t border-border bg-code px-8 py-3 text-xs leading-relaxed text-code-foreground">
        {JSON.stringify(entry.fields, null, 2)}
      </pre>
    </details>
  )
}

export function SystemLogsPage() {
  const { t, i18n } = useTranslation('settings')
  const desktop = isDesktopRuntime()
  const [paused, setPaused] = useState(false)
  const [query, setQuery] = useState('')
  const [entryLevel, setEntryLevel] = useState<EntryLevelFilter>('all')
  const [filterConfig, setFilterConfig] = useState<LogFilterConfig>({
    level: 'info',
    overrides: [],
  })
  const [actionError, setActionError] = useState<string | null>(null)
  const initialized = useRef(false)
  const appliedFilter = useRef(filterConfig)

  const logs = useQuery({
    queryKey: ['system-logs'],
    queryFn: getSystemLogs,
    enabled: desktop,
    refetchInterval: paused ? false : 1_000,
  })

  useEffect(() => {
    if (logs.data && !initialized.current) {
      const initial = parseLogFilter(logs.data.filter)
      setFilterConfig(initial)
      appliedFilter.current = initial
      initialized.current = true
    }
  }, [logs.data])

  useEffect(() => {
    document.title = t('logs.documentTitle')
  }, [i18n.resolvedLanguage, t])

  const updateFilter = useMutation({ mutationFn: setSystemLogFilter })
  const clearLogs = useMutation({
    mutationFn: clearSystemLogs,
    onSuccess: () => logs.refetch(),
  })

  const applyFilter = async (next: LogFilterConfig) => {
    setFilterConfig(next)
    setActionError(null)
    try {
      await updateFilter.mutateAsync(formatLogFilter(next))
      appliedFilter.current = next
      await logs.refetch()
    } catch (error) {
      setFilterConfig(appliedFilter.current)
      setActionError(error instanceof Error ? error.message : String(error))
    }
  }

  const visibleEntries = useMemo(() => {
    const needle = query.trim().toLowerCase()
    return (logs.data?.entries ?? []).filter((entry) => {
      const level = entry.level.toLowerCase()
      if (entryLevel !== 'all' && level !== entryLevel) return false
      if (!needle) return true
      return `${entry.message}\n${entry.target}\n${entry.level}\n${JSON.stringify(entry.fields)}`
        .toLowerCase()
        .includes(needle)
    })
  }, [entryLevel, logs.data?.entries, query])

  const onClear = async () => {
    setActionError(null)
    try {
      await clearLogs.mutateAsync()
    } catch (error) {
      setActionError(error instanceof Error ? error.message : String(error))
    }
  }

  const onOpenFolder = async () => {
    setActionError(null)
    try {
      await openSystemLogsFolder()
    } catch (error) {
      setActionError(error instanceof Error ? error.message : String(error))
    }
  }

  const busy = updateFilter.isPending || !desktop
  const locale = i18n.resolvedLanguage ?? i18n.language

  return (
    <DetailShell
      title={t('logs.title')}
      subtitle={t('logs.subtitle')}
      contentClassName="max-w-none"
    >
      <div className="space-y-10">
        {!desktop ? (
          <p className="rounded-md border border-border bg-muted px-4 py-3 text-sm text-muted-foreground">
            {t('logs.desktopRequired')}
          </p>
        ) : null}

        <SettingsSection title={t('logs.level.title')}>
          <SettingsRow
            label={t('logs.level.collection')}
            description={t('logs.level.description')}
            htmlFor="system-log-level"
          >
            <Select
              value={filterConfig.level}
              disabled={busy}
              onValueChange={(value) =>
                void applyFilter({ ...filterConfig, level: value as LogLevel })
              }
            >
              <SelectTrigger id="system-log-level">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {LOG_LEVELS.map((level) => (
                  <SelectItem key={level} value={level}>
                    {t(`logs.levels.${level}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </SettingsRow>
        </SettingsSection>

        <SettingsSection
          title={t('logs.overrides.title')}
          description={t('logs.overrides.description')}
          aside={
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={busy}
              onClick={() =>
                setFilterConfig((current) => ({
                  ...current,
                  overrides: [
                    ...current.overrides,
                    { target: '', level: 'debug' },
                  ],
                }))
              }
            >
              <Plus className="h-4 w-4" />
              {t('logs.overrides.add')}
            </Button>
          }
        >
          {filterConfig.overrides.length === 0 ? (
            <p className="py-4 text-sm text-muted-foreground">
              {t('logs.overrides.empty')}
            </p>
          ) : (
            filterConfig.overrides.map((override, index) => (
              <div
                key={index}
                className="grid gap-2 py-3 sm:grid-cols-[minmax(12rem,1fr)_10rem_auto]"
              >
                <Input
                  value={override.target}
                  disabled={busy}
                  aria-label={t('logs.overrides.module')}
                  placeholder="ag_swarmer_backend::api"
                  onChange={(event) =>
                    setFilterConfig((current) =>
                      withOverride(current, index, { target: event.target.value }),
                    )
                  }
                  onBlur={() => {
                    if (override.target.trim()) void applyFilter(filterConfig)
                  }}
                />
                <Select
                  value={override.level}
                  disabled={busy}
                  onValueChange={(value) => {
                    const next = withOverride(filterConfig, index, {
                      level: value as LogLevel,
                    })
                    if (override.target.trim()) void applyFilter(next)
                    else setFilterConfig(next)
                  }}
                >
                  <SelectTrigger aria-label={t('logs.overrides.level')}>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {LOG_LEVELS.map((level) => (
                      <SelectItem key={level} value={level}>
                        {t(`logs.levels.${level}`)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  disabled={busy}
                  aria-label={t('logs.overrides.remove')}
                  onClick={() => {
                    const next = {
                      ...filterConfig,
                      overrides: filterConfig.overrides.filter(
                        (_, overrideIndex) => overrideIndex !== index,
                      ),
                    }
                    if (override.target.trim()) void applyFilter(next)
                    else setFilterConfig(next)
                  }}
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            ))
          )}
        </SettingsSection>

        <Section
          title={t('logs.recent.title')}
          description={t('logs.recent.description')}
          aside={
            <div className="flex flex-wrap justify-end gap-2">
              <Button
                type="button"
                size="sm"
                disabled={!desktop}
                onClick={() => setPaused((value) => !value)}
              >
                {paused ? <Play className="h-4 w-4" /> : <Pause className="h-4 w-4" />}
                {t(paused ? 'logs.actions.resume' : 'logs.actions.pause')}
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={!desktop || logs.isFetching}
                onClick={() => void logs.refetch()}
              >
                <RefreshCw className={cn('h-4 w-4', logs.isFetching && 'animate-spin')} />
                {t('logs.actions.refresh')}
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={!desktop || clearLogs.isPending}
                onClick={() => void onClear()}
              >
                <Trash2 className="h-4 w-4" />
                {t('logs.actions.clear')}
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={!desktop}
                onClick={() => void onOpenFolder()}
              >
                <FolderOpen className="h-4 w-4" />
                {t('logs.actions.openFolder')}
              </Button>
            </div>
          }
          contentClassName="space-y-3"
        >
          <div className="flex flex-col gap-2 lg:flex-row lg:items-center">
            <div className="relative min-w-0 flex-1">
              <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
              <Input
                value={query}
                disabled={!desktop}
                onChange={(event) => setQuery(event.target.value)}
                placeholder={t('logs.recent.searchPlaceholder')}
                aria-label={t('logs.recent.search')}
                className="pl-9"
              />
            </div>
            <Select
              value={entryLevel}
              disabled={!desktop}
              onValueChange={(value) => setEntryLevel(value as EntryLevelFilter)}
            >
              <SelectTrigger className="lg:w-40" aria-label={t('logs.recent.levelFilter')}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {ENTRY_LEVELS.map((level) => (
                  <SelectItem key={level} value={level}>
                    {level === 'all'
                      ? t('logs.recent.allLevels')
                      : t(`logs.levels.${level}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="shrink-0 text-xs tabular-nums text-muted-foreground">
              {t('logs.recent.count', {
                shown: visibleEntries.length,
                total: logs.data?.entries.length ?? 0,
              })}
            </p>
          </div>

          {actionError ? (
            <p className="text-sm text-destructive" role="alert">
              {t('logs.errors.action', { message: actionError })}
            </p>
          ) : null}
          {logs.error ? (
            <p className="text-sm text-destructive" role="alert">
              {t('logs.errors.load', { message: String(logs.error) })}
            </p>
          ) : null}

          <div className="max-h-[34rem] overflow-auto rounded-md border border-border bg-card">
            {logs.isLoading ? (
              <p className="px-4 py-10 text-center text-sm text-muted-foreground">
                {t('logs.recent.loading')}
              </p>
            ) : visibleEntries.length === 0 ? (
              <p className="px-4 py-10 text-center text-sm text-muted-foreground">
                {query || entryLevel !== 'all'
                  ? t('logs.recent.noMatches')
                  : t('logs.recent.empty')}
              </p>
            ) : (
              visibleEntries.map((entry, index) => (
                <LogEntryRow
                  key={`${entry.timestamp}-${entry.target}-${index}`}
                  entry={entry}
                  locale={locale}
                />
              ))
            )}
          </div>

          {logs.data?.log_dir ? (
            <p className="truncate font-mono text-xs text-muted-foreground" title={logs.data.log_dir}>
              {logs.data.log_dir}
            </p>
          ) : null}
        </Section>
      </div>
    </DetailShell>
  )
}
