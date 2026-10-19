import { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { CalendarClock, History, KeyRound, Pencil, Plus, RefreshCw, Repeat2, Zap } from 'lucide-react'
import { Link } from 'react-router-dom'
import { useTranslation } from 'react-i18next'

import { AutomationStatusBadge } from '@/components/automations/AutomationStatusBadge'
import { Button } from '@/components/ui/button'
import { PageState } from '@/components/ui/page-state'
import { Switch } from '@/components/ui/switch'
import { useAutomations, useToggleAutomation } from '@/hooks/useAutomations'
import { useGroup } from '@/hooks/useGroups'
import { formatAutomationTime, formatRelativeRun } from '@/lib/automationTime'
import { cn } from '@/lib/utils'
import { AutomationFormDialog } from '@/pages/settings/AutomationFormDialog'
import { AutomationHistoryDialog } from '@/pages/settings/AutomationHistoryDialog'
import { AutomationSecretDialog } from '@/pages/settings/AutomationSecretDialog'
import type { AutomationRead, ConversationScope } from '@/types/api'

function GroupTriggers({ groupId }: { groupId: string }) {
  const { t, i18n } = useTranslation('automations')
  const locale = i18n.resolvedLanguage ?? 'en-US'
  const group = useGroup(groupId)
  const automations = useAutomations({ groupId })
  const toggle = useToggleAutomation()
  const queries = useQueryClient()
  const [editor, setEditor] = useState<{ automation: AutomationRead | null } | null>(null)
  const [history, setHistory] = useState<AutomationRead | null>(null)
  const [secret, setSecret] = useState<AutomationRead | null>(null)
  const [error, setError] = useState('')
  const items = (automations.data?.items ?? []).filter(item => item.group_id === groupId)
    .sort((a, b) => Number(b.enabled) - Number(a.enabled) || (a.next_run_at ?? Infinity) - (b.next_run_at ?? Infinity))
  const active = items.filter(item => item.enabled).length
  const refresh = () => { void automations.refetch(); void group.refetch() }

  const setEnabled = (item: AutomationRead, enabled: boolean) => {
    setError('')
    toggle.mutate({ id: item.id, input: { enabled, expected_version: item.version } }, {
      onError: failure => {
        setError(failure.message)
        void queries.invalidateQueries({ queryKey: ['automations', groupId] })
      },
    })
  }

  return <div className="flex h-full min-h-0 flex-col">
    <div className="flex min-h-12 shrink-0 items-center gap-2 border-b border-border px-3 py-2">
      <CalendarClock className="h-4 w-4 text-muted-foreground" aria-hidden />
      <p className="min-w-0 flex-1 truncate text-xs font-medium">{t('panel.title')}</p>
      <Button size="icon" variant="ghost" className="h-7 w-7" aria-label={t('panel.refresh')} onClick={refresh} disabled={automations.isFetching}><RefreshCw className="h-3.5 w-3.5" aria-hidden /></Button>
      <Button size="sm" variant="ghost" className="h-7 gap-1 px-2" aria-label={t('new')} onClick={() => setEditor({ automation: null })} disabled={!group.data}><Plus className="h-3.5 w-3.5" aria-hidden />{t('panel.new')}</Button>
    </div>
    <div className="min-h-0 flex-1 overflow-y-auto">
      {automations.isLoading || group.isLoading ? <PageState variant="loading" title={t('preview.loading')} className="p-5" /> : automations.error || group.error ? (
        <PageState variant="error" title={t('errors.load', { message: (automations.error || group.error)?.message })} className="p-5" action={<Button size="sm" variant="outline" onClick={refresh}>{t('panel.refresh')}</Button>} />
      ) : <>
        <div className="space-y-1 border-b border-border/60 px-3 py-3">
          <p className="text-xs font-medium">{t('panel.active', { count: active })}</p>
          <p className="text-xs leading-relaxed text-muted-foreground">{t('panel.description')}</p>
        </div>
        {error ? <p role="alert" className="px-3 pt-3 text-xs text-destructive">{error}</p> : null}
        {items.length === 0 ? <PageState icon={CalendarClock} title={t('emptyFiltered')} description={t('emptyBody')} className="min-h-64 p-5 [&_p]:text-xs" action={<Button size="sm" onClick={() => setEditor({ automation: null })} disabled={!group.data}><Plus className="h-3.5 w-3.5" aria-hidden />{t('new')}</Button>} /> : (
          <ul className="divide-y divide-border/60">
            {items.map(item => {
              const eventDriven = item.trigger.kind === 'webhook' || item.trigger.kind === 'github'
              const relative = formatRelativeRun(item.next_run_at, locale)
              return <li key={item.id} className={cn('space-y-3 px-3 py-4 transition-colors hover:bg-muted/20', !item.enabled && 'bg-muted/15')}>
                <div className="flex items-start gap-2.5">
                  <span className={cn('mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-md', item.enabled ? 'bg-primary/10 text-primary' : 'bg-muted text-muted-foreground')}>
                    {eventDriven ? <Zap className="h-3.5 w-3.5" aria-hidden /> : item.trigger.kind === 'cron' ? <Repeat2 className="h-3.5 w-3.5" aria-hidden /> : <CalendarClock className="h-3.5 w-3.5" aria-hidden />}
                  </span>
                  <div className="min-w-0 flex-1">
                    <button className="max-w-full truncate text-left text-xs font-semibold hover:text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" onClick={() => setEditor({ automation: item })}>{item.name}</button>
                    <p className="mt-1 truncate text-2xs text-muted-foreground">{item.trigger.kind === 'cron' ? <><span className="font-mono">{item.trigger.expression}</span> · {item.timezone}</> : item.trigger.kind === 'once' ? t('card.once', { when: formatAutomationTime(item.trigger.at, locale, item.timezone) }) : item.trigger.kind === 'github' ? item.trigger.events.length ? t('card.github', { events: item.trigger.events.join(', ') }) : t('card.githubAll') : t('card.webhook')}</p>
                  </div>
                  <Switch className="mt-0.5 shrink-0" checked={item.enabled} disabled={toggle.isPending} aria-label={`${t(item.enabled ? 'card.pause' : 'card.resume')} ${item.name}`} onCheckedChange={enabled => setEnabled(item, enabled)} />
                </div>
                <div className="space-y-1.5 border-l border-border pl-3 text-xs">
                  <p className="text-muted-foreground">{!item.enabled ? t('card.paused') : eventDriven ? t('card.eventDriven') : item.next_run_at === null ? t('card.noNextRun') : t('card.nextRun', { when: formatAutomationTime(item.next_run_at, locale, item.timezone) })}</p>
                  {item.enabled && relative ? <p className="text-2xs text-muted-foreground/80">{relative}</p> : null}
                  <div className="flex flex-wrap items-center gap-1.5 text-muted-foreground"><span>{t('card.lastRun')}</span>{item.latest?.status ? <AutomationStatusBadge status={item.latest.status} /> : <span>{t('card.neverRun')}</span>}</div>
                  {item.latest?.error ? <p className="break-words text-xs text-destructive">{item.latest.error}</p> : null}
                </div>
                <div className="flex flex-wrap gap-1">
                  <Button size="sm" variant="ghost" className="h-7 gap-1 px-2" onClick={() => setEditor({ automation: item })}><Pencil className="h-3 w-3" aria-hidden />{t('card.edit')}</Button>
                  <Button size="sm" variant="ghost" className="h-7 gap-1 px-2" onClick={() => setHistory(item)}><History className="h-3 w-3" aria-hidden />{t('card.history')}</Button>
                  {eventDriven ? <Button size="sm" variant="ghost" className="h-7 gap-1 px-2" onClick={() => setSecret(item)}><KeyRound className="h-3 w-3" aria-hidden />{t('card.secret')}</Button> : null}
                </div>
              </li>
            })}
          </ul>
        )}
      </>}
    </div>
    {editor && group.data ? <AutomationFormDialog key={editor.automation?.id ?? 'new'} open automation={editor.automation} groups={[group.data]} onOpenChange={open => { if (!open) setEditor(null) }} onSaved={saved => { setEditor(null); if ((saved.trigger.kind === 'webhook' || saved.trigger.kind === 'github') && !saved.hook_configured) setSecret(saved) }} /> : null}
    {history ? <AutomationHistoryDialog open automation={history} onOpenChange={open => { if (!open) setHistory(null) }} /> : null}
    {secret ? <AutomationSecretDialog open automation={secret} onOpenChange={open => { if (!open) setSecret(null) }} onChanged={() => { void automations.refetch() }} /> : null}
  </div>
}

export function ConversationTriggersTab({ scope, conversationId }: { scope: ConversationScope; conversationId: string | undefined }) {
  const { t } = useTranslation('automations')
  if (scope !== 'groups' || !conversationId) return <PageState icon={CalendarClock} title={t('panel.groupOnlyTitle')} description={t('panel.groupOnlyBody')} className="p-5 [&_p]:text-xs" action={<Button size="sm" variant="outline" asChild><Link to="/settings/automations">{t('panel.manage')}</Link></Button>} />
  return <GroupTriggers key={conversationId} groupId={conversationId} />
}
