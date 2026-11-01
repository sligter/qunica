import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Card } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { useEngineCapabilities, useEngineMetrics, usePauseRig } from '@/hooks/useEngineRollout'

export function EngineCanaryPanel() {
  const { t } = useTranslation('usage')
  const [days, setDays] = useState(7)
  const capabilities = useEngineCapabilities()
  const metrics = useEngineMetrics(days)
  const pause = usePauseRig()
  const percent = (value: number | null) => value === null ? '—' : `${(value * 100).toFixed(1)}%`
  return <Card className="space-y-4 p-5">
    <div className="flex flex-wrap items-start justify-between gap-3">
      <div><h2 className="text-base font-semibold">{t('canary.title')}</h2>
        <p className="mt-1 max-w-3xl text-xs text-muted-foreground">{t('canary.description')}</p></div>
      <div className="flex flex-wrap gap-2">
        <select aria-label={t('canary.window')} value={days} onChange={event => setDays(Number(event.target.value))}
          className="h-9 rounded-md border border-input bg-background px-3 text-sm">
          {[7, 30, 90].map(value => <option key={value} value={value}>{t('canary.days', { count: value })}</option>)}
        </select>
        <Button variant="outline" size="sm" onClick={() => void metrics.refetch()} disabled={metrics.isFetching}>{t('refresh')}</Button>
        <Button variant="outline" size="sm" disabled={!capabilities.data?.rig_available || pause.isPending}
          onClick={() => pause.mutate(!capabilities.data?.rig_paused)}>
          {capabilities.data?.rig_paused ? t('canary.resume') : t('canary.pause')}
        </Button>
      </div>
    </div>
    {capabilities.data?.rig_paused && <p role="status" className="text-sm text-warning-foreground">{t('canary.paused')}</p>}
    {(pause.isError || metrics.isError || capabilities.isError) && <p role="alert" className="text-sm text-destructive">{t('canary.error')}</p>}
    {metrics.isPending && <p className="text-sm text-muted-foreground">{t('canary.loading')}</p>}
    {metrics.data && metrics.data.rows.length === 0 && <p className="text-sm text-muted-foreground">{t('canary.empty')}</p>}
    {Boolean(metrics.data?.rows.length) && <div className="overflow-x-auto"><table className="w-full text-left text-sm">
      <thead className="border-b text-xs text-muted-foreground"><tr>
        {['engine', 'version', 'model', 'completed', 'failureRate', 'tokens', 'toolErrors', 'pending'].map(key => <th className="px-2 py-2 font-medium" key={key}>{t(`canary.${key}`)}</th>)}
      </tr></thead>
      <tbody>{metrics.data?.rows.map((row, index) => <tr key={`${row.engine}-${index}`} className="border-b last:border-0">
        <td className="px-2 py-3 font-medium">{row.engine === 'rig' ? 'Rig' : t('canary.native')}</td>
        <td className="px-2 py-3 text-xs">{row.app_version} / {row.engine_version}</td>
        <td className="max-w-56 truncate px-2 py-3" title={`${row.provider_id ?? ''} ${row.provider_kind} ${row.model}`}>
          <div>{row.provider_kind} · {row.model || '—'}</div>
          <div className="text-xs text-muted-foreground">{row.provider_id ?? '—'}</div>
        </td>
        <td className="px-2 py-3 tabular-nums">{row.completed} / {row.completed + row.failed}</td>
        <td className="px-2 py-3 tabular-nums">{percent(row.failure_rate)}</td>
        <td className="px-2 py-3 tabular-nums">{row.usage_calls ? row.total_tokens.toLocaleString() : '—'}{row.executions_without_usage > 0 ? '*' : ''}</td>
        <td className="px-2 py-3 tabular-nums">{row.tool_errors} / {row.settled_tool_calls} ({percent(row.tool_error_rate)})</td>
        <td className="px-2 py-3 text-xs">{t('canary.pendingCounts', { pending: row.pending, cancelled: row.cancelled, interrupted: row.interrupted, blocked: row.blocked, unknown: row.outcome_unknown, partial: row.partial_executions })}</td>
      </tr>)}</tbody>
    </table></div>}
    <p className="text-xs text-muted-foreground">{t('canary.method')}</p>
    <p className="text-xs text-muted-foreground">{t('canary.coverage', { count: metrics.data?.unattributed_calls ?? 0 })}</p>
  </Card>
}
