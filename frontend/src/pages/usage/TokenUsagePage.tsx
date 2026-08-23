import { useEffect, useMemo, useState, type ComponentType } from 'react'
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  Bot,
  Braces,
  Coins,
  Layers3,
  RefreshCw,
  SlidersHorizontal,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { DetailShell } from '@/components/layout/DetailShell'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useTokenUsage, type TokenUsageFilters } from '@/hooks/useTokenUsage'
import { normalizeLanguage } from '@/i18n'
import { cn } from '@/lib/utils'
import type {
  TokenUsageBreakdown,
  TokenUsageFilterOption,
  TokenUsageTimelinePoint,
} from '@/types/api'

type RangePreset = 'today' | 'yesterday' | '7d' | '30d' | '90d' | 'custom'
type Dimension = 'group' | 'provider' | 'model' | 'agent'

interface UsageSeries {
  id: string
  name: string
  points: TokenUsageTimelinePoint[]
}

const ALL = '__all__'

const SERIES_COLORS = [
  'var(--color-primary)',
  'var(--color-avatar-3)',
  'var(--color-avatar-5)',
  'var(--color-avatar-2)',
  'var(--color-avatar-4)',
  'var(--color-avatar-6)',
  'var(--color-avatar-7)',
  'var(--color-avatar-8)',
]

const RANGE_PRESETS: RangePreset[] = ['today', 'yesterday', '7d', '30d', '90d', 'custom']

function dateInputValue(date: Date) {
  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  return `${year}-${month}-${day}`
}

function daysAgo(days: number) {
  const date = new Date()
  date.setDate(date.getDate() - days)
  return date
}

/** The inclusive `from`/`to` pair a preset stands for. */
function presetDates(preset: Exclude<RangePreset, 'custom'>) {
  if (preset === 'today' || preset === 'yesterday') {
    const day = dateInputValue(daysAgo(preset === 'today' ? 0 : 1))
    return { from: day, to: day }
  }
  const days = Number.parseInt(preset, 10)
  return { from: dateInputValue(daysAgo(days - 1)), to: dateInputValue(new Date()) }
}

function formatTokens(value: number, locale: string, compact = false) {
  return new Intl.NumberFormat(locale, compact
    ? { notation: 'compact', maximumFractionDigits: 1 }
    : undefined).format(value)
}

function FilterSelect({
  label,
  value,
  options,
  allLabel,
  onChange,
}: {
  label: string
  value?: string
  options: TokenUsageFilterOption[]
  allLabel: string
  onChange: (value?: string) => void
}) {
  return (
    <label className="min-w-0 space-y-1.5">
      <span className="text-2xs font-semibold uppercase tracking-[0.16em] text-muted-foreground">
        {label}
      </span>
      <Select value={value ?? ALL} onValueChange={(next) => onChange(next === ALL ? undefined : next)}>
        <SelectTrigger className="bg-background shadow-none">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={ALL}>{allLabel}</SelectItem>
          {options.map((option) => (
            <SelectItem key={option.id} value={option.id}>{option.name}</SelectItem>
          ))}
        </SelectContent>
      </Select>
    </label>
  )
}

function MetricCard({
  icon: Icon,
  label,
  value,
  note,
  tone,
}: {
  icon: ComponentType<{ className?: string }>
  label: string
  value: string
  note: string
  tone: string
}) {
  return (
    <Card className="relative overflow-hidden p-5">
      <div className={cn('absolute inset-x-0 top-0 h-0.5', tone)} />
      <div className="flex items-start justify-between gap-3">
        <div>
          <p className="text-xs font-medium text-muted-foreground">{label}</p>
          <p className="mt-2 font-serif text-3xl font-semibold tabular-nums tracking-tight">{value}</p>
          <p className="mt-1 text-2xs text-muted-foreground">{note}</p>
        </div>
        <span className="rounded-full border border-border bg-muted/60 p-2.5 text-muted-foreground">
          <Icon className="h-4 w-4" />
        </span>
      </div>
    </Card>
  )
}

function UsageChart({
  series,
  locale,
  emptyLabel,
  ariaLabel,
  area = false,
  legend = false,
}: {
  series: UsageSeries[]
  locale: string
  emptyLabel: string
  ariaLabel: string
  area?: boolean
  legend?: boolean
}) {
  const dates = [...new Set(series.flatMap((item) => item.points.map((point) => point.date)))].sort()
  if (dates.length === 0) {
    return <div className="flex h-64 items-center justify-center text-sm text-muted-foreground">{emptyLabel}</div>
  }

  const width = 960
  const height = 250
  const left = 54
  const right = 18
  const top = 18
  const bottom = 34
  const chartWidth = width - left - right
  const chartHeight = height - top - bottom
  const plotted = series.map((item, index) => ({
    ...item,
    color: SERIES_COLORS[index % SERIES_COLORS.length],
    total: item.points.reduce((sum, point) => sum + point.total_tokens, 0),
    values: new Map(item.points.map((point) => [point.date, point.total_tokens])),
  }))
  const max = Math.max(...plotted.flatMap((item) => [...item.values.values()]), 1)
  const x = (index: number) => left + (dates.length === 1 ? chartWidth / 2 : (index / (dates.length - 1)) * chartWidth)
  const y = (value: number) => top + chartHeight - (value / max) * chartHeight
  const path = (values: Map<string, number>) => dates
    .map((date, index) => `${index ? 'L' : 'M'} ${x(index)} ${y(values.get(date) ?? 0)}`)
    .join(' ')
  const labelIndexes = [...new Set([0, Math.floor((dates.length - 1) / 2), dates.length - 1])]
  const date = new Intl.DateTimeFormat(locale, { month: 'short', day: 'numeric' })
  const firstPath = path(plotted[0].values)

  return (
    <div>
      <svg className="h-64 w-full overflow-visible" viewBox={`0 0 ${width} ${height}`} role="img" aria-label={ariaLabel}>
        <defs>
          <linearGradient id="token-usage-area" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="var(--color-primary)" stopOpacity="0.28" />
            <stop offset="1" stopColor="var(--color-primary)" stopOpacity="0.02" />
          </linearGradient>
        </defs>
        {[0, 0.5, 1].map((ratio) => (
          <g key={ratio}>
            <line
              x1={left}
              x2={width - right}
              y1={top + chartHeight * ratio}
              y2={top + chartHeight * ratio}
              stroke="var(--color-border)"
              strokeDasharray="3 6"
            />
            <text
              x={left - 10}
              y={top + chartHeight * ratio + 4}
              textAnchor="end"
              fill="var(--color-muted-foreground)"
              fontSize="11"
            >
              {formatTokens(Math.round(max * (1 - ratio)), locale, true)}
            </text>
          </g>
        ))}
        {area ? (
          <path d={`${firstPath} L ${x(dates.length - 1)} ${top + chartHeight} L ${x(0)} ${top + chartHeight} Z`} fill="url(#token-usage-area)" />
        ) : null}
        {plotted.map((item) => (
          <g key={item.id}>
            <path d={path(item.values)} fill="none" stroke={item.color} strokeWidth="2.5" strokeLinejoin="round" strokeLinecap="round" />
            {dates.map((day, index) => {
              const value = item.values.get(day) ?? 0
              return (
                <circle key={day} cx={x(index)} cy={y(value)} r="3" fill="var(--color-card)" stroke={item.color} strokeWidth="2">
                  <title>{`${item.name} · ${day}: ${formatTokens(value, locale)}`}</title>
                </circle>
              )
            })}
          </g>
        ))}
        {labelIndexes.map((index) => (
          <text
            key={dates[index]}
            x={x(index)}
            y={height - 8}
            textAnchor={index === 0 ? 'start' : index === dates.length - 1 ? 'end' : 'middle'}
            fill="var(--color-muted-foreground)"
            fontSize="11"
          >
            {date.format(new Date(`${dates[index]}T00:00:00`))}
          </text>
        ))}
      </svg>
      {legend ? (
        <ul className="mt-3 flex flex-wrap gap-x-5 gap-y-2 border-t border-border pt-4" aria-label={ariaLabel}>
          {plotted.map((item) => (
            <li key={item.id} className="flex min-w-32 items-center gap-2 text-xs">
              <span className="h-2 w-2 shrink-0 rounded-full" style={{ backgroundColor: item.color }} aria-hidden />
              <span className="max-w-48 truncate text-muted-foreground" title={item.name}>{item.name}</span>
              <span className="ml-auto font-medium tabular-nums">{formatTokens(item.total, locale, true)}</span>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  )
}

function BreakdownList({
  items,
  total,
  locale,
  emptyLabel,
  callsLabel,
  onSelect,
}: {
  items: TokenUsageBreakdown[]
  total: number
  locale: string
  emptyLabel: string
  callsLabel: (count: number) => string
  onSelect: (id: string) => void
}) {
  if (items.length === 0) {
    return <p className="py-12 text-center text-sm text-muted-foreground">{emptyLabel}</p>
  }
  return (
    <div className="divide-y divide-border">
      {items.map((item, index) => {
        const share = total > 0 ? (item.total_tokens / total) * 100 : 0
        return (
          <button
            key={item.id}
            type="button"
            className="group grid w-full grid-cols-[2rem_minmax(0,1fr)_auto] items-center gap-3 py-4 text-left"
            onClick={() => onSelect(item.id)}
          >
            <span className="font-serif text-sm tabular-nums text-muted-foreground">{String(index + 1).padStart(2, '0')}</span>
            <span className="min-w-0">
              <span className="flex items-center justify-between gap-3">
                <span className="truncate text-sm font-medium group-hover:text-primary">{item.name}</span>
                <span className="text-2xs tabular-nums text-muted-foreground">{share.toFixed(1)}%</span>
              </span>
              <span className="mt-2 block h-1.5 overflow-hidden rounded-full bg-muted">
                <span className="block h-full rounded-full bg-primary/70" style={{ width: `${Math.max(share, 0.8)}%` }} />
              </span>
            </span>
            <span className="min-w-24 text-right">
              <span className="block text-sm font-semibold tabular-nums">{formatTokens(item.total_tokens, locale, true)}</span>
              <span className="text-2xs text-muted-foreground">{callsLabel(item.calls)}</span>
            </span>
          </button>
        )
      })}
    </div>
  )
}

export function TokenUsagePage() {
  const { t, i18n } = useTranslation(['usage', 'navigation'])
  const locale = normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US'
  // Today by default: the page exists to answer "what has this cost me", and
  // the answer the user is after is almost always about right now, not a
  // month ago.
  const initialDates = useMemo(() => presetDates('today'), [])
  const [range, setRange] = useState<RangePreset>('today')
  const [filters, setFilters] = useState<TokenUsageFilters>(initialDates)
  const validRange = Boolean(filters.from && filters.to && filters.from <= filters.to)
  const usage = useTokenUsage(filters, validRange)
  const data = usage.data

  useEffect(() => {
    document.title = t('documentTitle')
  }, [t])

  const selectRange = (next: RangePreset) => {
    setRange(next)
    if (next !== 'custom') {
      setFilters((current) => ({ ...current, ...presetDates(next) }))
    }
  }

  const setFilter = (key: keyof TokenUsageFilters, value?: string) => {
    setFilters((current) => ({ ...current, [key]: value || undefined }))
  }

  const clearDimensions = () => {
    setFilters(({ from, to }) => ({ from, to }))
  }

  const applyBreakdown = (dimension: Dimension, id: string) => {
    setFilter(`${dimension}_id` as keyof TokenUsageFilters, id)
  }

  const summary = data?.summary ?? {
    total_tokens: 0,
    input_tokens: 0,
    output_tokens: 0,
    calls: 0,
    active_agents: 0,
  }
  const dimensionFilters = filters.group_id || filters.provider_id || filters.model || filters.agent_id
  const breakdowns: [Dimension, TokenUsageBreakdown[]][] = [
    ['group', data?.by_group ?? []],
    ['provider', data?.by_provider ?? []],
    ['model', data?.by_model ?? []],
    ['agent', data?.by_agent ?? []],
  ]

  return (
    <DetailShell
      title={t('title')}
      subtitle={t('subtitle')}
      measure="wide"
      contentClassName="max-w-[90rem]"
      actions={
        <Button variant="outline" size="sm" disabled={usage.isFetching || !validRange} onClick={() => void usage.refetch()}>
          <RefreshCw className={cn('h-4 w-4', usage.isFetching && 'animate-spin')} />
          {t('refresh')}
        </Button>
      }
    >
      <div className="space-y-6">
        <section className="overflow-hidden rounded-xl border border-border bg-card shadow-sm">
          <div className="flex items-center justify-between gap-3 border-b border-border bg-muted/30 px-5 py-3">
            <div className="flex items-center gap-2 text-sm font-medium">
              <SlidersHorizontal className="h-4 w-4 text-primary" />
              {t('filters.title')}
            </div>
            {dimensionFilters ? (
              <button type="button" className="text-xs font-medium text-primary hover:underline" onClick={clearDimensions}>
                {t('filters.clear')}
              </button>
            ) : null}
          </div>
          <div className="grid gap-4 p-5 md:grid-cols-2 xl:grid-cols-6">
            <div className="space-y-1.5 md:col-span-2">
              <span className="text-2xs font-semibold uppercase tracking-[0.16em] text-muted-foreground">{t('filters.range')}</span>
              <div className="grid h-9 grid-cols-6 rounded-md border border-input bg-background p-0.5">
                {RANGE_PRESETS.map((preset) => (
                  <button
                    key={preset}
                    type="button"
                    aria-pressed={range === preset}
                    className={cn('truncate rounded px-1 text-xs font-medium transition-colors', range === preset ? 'bg-primary text-primary-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground')}
                    onClick={() => selectRange(preset)}
                  >
                    {t(`ranges.${preset}`)}
                  </button>
                ))}
              </div>
            </div>
            <FilterSelect label={t('filters.group')} value={filters.group_id} options={data?.filters.groups ?? []} allLabel={t('filters.allGroups')} onChange={(value) => setFilter('group_id', value)} />
            <FilterSelect label={t('filters.provider')} value={filters.provider_id} options={data?.filters.providers ?? []} allLabel={t('filters.allProviders')} onChange={(value) => setFilter('provider_id', value)} />
            <FilterSelect label={t('filters.model')} value={filters.model} options={data?.filters.models ?? []} allLabel={t('filters.allModels')} onChange={(value) => setFilter('model', value)} />
            <FilterSelect label={t('filters.agent')} value={filters.agent_id} options={data?.filters.agents ?? []} allLabel={t('filters.allAgents')} onChange={(value) => setFilter('agent_id', value)} />
          </div>
          {range === 'custom' ? (
            <div className="flex flex-wrap items-end gap-3 border-t border-border px-5 py-4">
              <label className="space-y-1.5">
                <span className="block text-2xs font-semibold uppercase tracking-[0.16em] text-muted-foreground">{t('filters.from')}</span>
                <Input type="date" value={filters.from} max={filters.to} onChange={(event) => setFilters((current) => ({ ...current, from: event.target.value }))} />
              </label>
              <label className="space-y-1.5">
                <span className="block text-2xs font-semibold uppercase tracking-[0.16em] text-muted-foreground">{t('filters.to')}</span>
                <Input type="date" value={filters.to} min={filters.from} max={dateInputValue(new Date())} onChange={(event) => setFilters((current) => ({ ...current, to: event.target.value }))} />
              </label>
              {!validRange ? <p className="pb-2 text-sm text-destructive" role="alert">{t('filters.invalidRange')}</p> : null}
            </div>
          ) : null}
        </section>

        {usage.error ? (
          <p className="rounded-lg border border-destructive/30 bg-destructive/5 px-4 py-3 text-sm text-destructive" role="alert">
            {t('loadError', { message: String(usage.error) })}
          </p>
        ) : null}

        <section className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4" aria-label={t('summary.title')}>
          <MetricCard icon={Coins} label={t('summary.total')} value={formatTokens(summary.total_tokens, locale, true)} note={t('summary.totalNote')} tone="bg-primary" />
          <MetricCard icon={ArrowDownToLine} label={t('summary.input')} value={formatTokens(summary.input_tokens, locale, true)} note={t('summary.inputNote')} tone="bg-avatar-3" />
          <MetricCard icon={ArrowUpFromLine} label={t('summary.output')} value={formatTokens(summary.output_tokens, locale, true)} note={t('summary.outputNote')} tone="bg-avatar-5" />
          <MetricCard icon={Bot} label={t('summary.calls')} value={formatTokens(summary.calls, locale)} note={t('summary.agentCount', { count: summary.active_agents })} tone="bg-avatar-2" />
        </section>

        <Card asChild>
          <section className="overflow-hidden">
            <header className="flex flex-wrap items-end justify-between gap-3 border-b border-border px-6 py-5">
              <div>
                <p className="text-2xs font-semibold uppercase tracking-[0.2em] text-primary">{t('trend.eyebrow')}</p>
                <h2 className="mt-1 font-serif text-xl font-semibold">{t('trend.title')}</h2>
              </div>
              <p className="text-xs text-muted-foreground">{filters.from} — {filters.to}</p>
            </header>
            <div className="px-4 pb-2 pt-5 sm:px-6">
              {usage.isLoading ? (
                <div className="flex h-64 items-center justify-center text-sm text-muted-foreground">{t('loading')}</div>
              ) : (
                <UsageChart
                  series={[{ id: 'total', name: t('summary.total'), points: data?.timeline ?? [] }]}
                  locale={locale}
                  emptyLabel={t('empty')}
                  ariaLabel={t('trend.ariaLabel')}
                  area
                />
              )}
            </div>
          </section>
        </Card>

        <Card asChild>
          <section className="overflow-hidden">
            <header className="flex flex-wrap items-end justify-between gap-3 border-b border-border px-6 py-5">
              <div>
                <p className="text-2xs font-semibold uppercase tracking-[0.2em] text-primary">{t('attributionTrend.eyebrow')}</p>
                <h2 className="mt-1 font-serif text-xl font-semibold">{t('attributionTrend.title')}</h2>
                <p className="mt-1 text-sm text-muted-foreground">{t('attributionTrend.description')}</p>
              </div>
              <p className="text-xs text-muted-foreground">{filters.from} — {filters.to}</p>
            </header>
            <Tabs defaultValue="group" className="p-6">
              <TabsList className="grid w-full grid-cols-4 sm:w-auto">
                {breakdowns.map(([dimension]) => (
                  <TabsTrigger key={dimension} value={dimension}>{t(`dimensions.${dimension}`)}</TabsTrigger>
                ))}
              </TabsList>
              {breakdowns.map(([dimension, items]) => (
                <TabsContent key={dimension} value={dimension} className="mt-4">
                  {usage.isLoading ? (
                    <div className="flex h-64 items-center justify-center text-sm text-muted-foreground">{t('loading')}</div>
                  ) : (
                    <UsageChart
                      series={items.map((item) => ({ id: item.id, name: item.name, points: item.timeline }))}
                      locale={locale}
                      emptyLabel={t('empty')}
                      ariaLabel={t('attributionTrend.ariaLabel', { dimension: t(`dimensions.${dimension}`) })}
                      legend
                    />
                  )}
                </TabsContent>
              ))}
            </Tabs>
          </section>
        </Card>

        <Card asChild>
          <section className="overflow-hidden">
            <div className="border-b border-border px-6 py-5">
              <p className="text-2xs font-semibold uppercase tracking-[0.2em] text-primary">{t('breakdown.eyebrow')}</p>
              <h2 className="mt-1 font-serif text-xl font-semibold">{t('breakdown.title')}</h2>
              <p className="mt-1 text-sm text-muted-foreground">{t('breakdown.description')}</p>
            </div>
            <Tabs defaultValue="group" className="p-6">
              <TabsList className="grid w-full grid-cols-4 sm:w-auto">
                <TabsTrigger value="group"><Layers3 className="mr-1.5 h-3.5 w-3.5" />{t('dimensions.group')}</TabsTrigger>
                <TabsTrigger value="provider"><Braces className="mr-1.5 h-3.5 w-3.5" />{t('dimensions.provider')}</TabsTrigger>
                <TabsTrigger value="model">{t('dimensions.model')}</TabsTrigger>
                <TabsTrigger value="agent">{t('dimensions.agent')}</TabsTrigger>
              </TabsList>
              {breakdowns.map(([dimension, items]) => (
                <TabsContent key={dimension} value={dimension} className="mt-4">
                  <BreakdownList
                    items={items}
                    total={summary.total_tokens}
                    locale={locale}
                    emptyLabel={t('empty')}
                    callsLabel={(count) => t('breakdown.calls', { count })}
                    onSelect={(id) => dimension === 'model' ? setFilter('model', id) : applyBreakdown(dimension, id)}
                  />
                </TabsContent>
              ))}
            </Tabs>
          </section>
        </Card>
      </div>
    </DetailShell>
  )
}
