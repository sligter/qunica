import { useMemo, useState } from 'react'
import {
  CalendarClock,
  History,
  KeyRound,
  Pencil,
  Plus,
  Repeat2,
  Sparkles,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { AutomationStatusBadge } from '@/components/automations/AutomationStatusBadge'
import { DetailShell } from '@/components/layout/DetailShell'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { PageState } from '@/components/ui/page-state'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { useAutomations, useToggleAutomation } from '@/hooks/useAutomations'
import { useGroups } from '@/hooks/useGroups'
import {
  formatAutomationTime,
  formatRelativeRun,
} from '@/lib/automationTime'
import { ApiError } from '@/lib/api-v2/client'
import { cn } from '@/lib/utils'
import { AutomationFormDialog } from '@/pages/settings/AutomationFormDialog'
import { AutomationHistoryDialog } from '@/pages/settings/AutomationHistoryDialog'
import { AutomationSecretDialog } from '@/pages/settings/AutomationSecretDialog'
import type { AutomationRead } from '@/types/api'

const ALL_GROUPS = '__all__'

export function AutomationsPage() {
  const { t, i18n } = useTranslation('automations')
  const locale = i18n.resolvedLanguage ?? 'en-US'
  const groupsQuery = useGroups()
  const groups = useMemo(() => groupsQuery.data ?? [], [groupsQuery.data])
  const [groupFilter, setGroupFilter] = useState(ALL_GROUPS)
  const automationsQuery = useAutomations(
    groupFilter === ALL_GROUPS ? {} : { groupId: groupFilter },
  )
  const toggle = useToggleAutomation()
  const [editor, setEditor] = useState<{ automation: AutomationRead | null } | null>(null)
  const [secretTarget, setSecretTarget] = useState<AutomationRead | null>(null)
  const [historyTarget, setHistoryTarget] = useState<AutomationRead | null>(null)
  const [toggleError, setToggleError] = useState<string | null>(null)

  const groupNames = useMemo(
    () => new Map(groups.map((group) => [group.id, group.name])),
    [groups],
  )
  const automations = automationsQuery.data?.items ?? []
  const hasGroups = groups.length > 0
  const filtered = groupFilter !== ALL_GROUPS

  const triggerLabel = (automation: AutomationRead): string => {
    const trigger = automation.trigger
    if (trigger.kind === 'cron') {
      return t('card.cron', { expression: trigger.expression })
    }
    if (trigger.kind === 'once') {
      return t('card.once', {
        when: formatAutomationTime(trigger.at, locale),
      })
    }
    if (trigger.kind === 'github') {
      return trigger.events.length > 0
        ? t('card.github', { events: trigger.events.join(', ') })
        : t('card.githubAll')
    }
    return t('card.webhook')
  }

  const setEnabled = (automation: AutomationRead, enabled: boolean) => {
    setToggleError(null)
    toggle.mutate(
      {
        id: automation.id,
        input: { enabled, expected_version: automation.version },
      },
      {
        onError: (error) => {
          setToggleError(
            error instanceof ApiError && error.code === 'conflict'
              ? t('errors.conflict')
              : error instanceof ApiError
                ? t('errors.toggle', { message: error.message })
                : t('errors.network'),
          )
        },
      },
    )
  }

  return (
    <DetailShell
      title={t('title')}
      subtitle={t('subtitle')}
      actions={
        hasGroups ? (
          <Button
            type="button"
            size="sm"
            onClick={() => setEditor({ automation: null })}
          >
            <Plus className="h-4 w-4" aria-hidden />
            {t('new')}
          </Button>
        ) : null
      }
    >
      {automationsQuery.isLoading || groupsQuery.isLoading ? (
        <PageState variant="loading" title={t('preview.loading')} />
      ) : automationsQuery.error ? (
        <PageState
          variant="error"
          title={t('errors.load', { message: String(automationsQuery.error) })}
        />
      ) : groupsQuery.error ? (
        <PageState
          variant="error"
          title={t('errors.load', { message: String(groupsQuery.error) })}
        />
      ) : !hasGroups ? (
        <PageState
          variant="empty"
          icon={CalendarClock}
          title={t('noGroupsTitle')}
          description={t('noGroupsBody')}
        />
      ) : automations.length === 0 ? (
        <PageState
          variant="empty"
          icon={filtered ? Sparkles : CalendarClock}
          title={filtered ? t('emptyFiltered') : t('emptyTitle')}
          description={filtered ? undefined : t('emptyBody')}
          action={
            filtered ? (
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => setGroupFilter(ALL_GROUPS)}
              >
                {t('filterAllGroups')}
              </Button>
            ) : (
              <Button type="button" size="sm" onClick={() => setEditor({ automation: null })}>
                <Plus className="h-4 w-4" aria-hidden />
                {t('new')}
              </Button>
            )
          }
        />
      ) : (
        <div className="space-y-4">
          <div className="flex flex-wrap items-center gap-3">
            <p className="text-xs tabular-nums text-muted-foreground">
              {t('count', { count: automations.length })}
            </p>
            {groups.length > 1 ? (
              <Select value={groupFilter} onValueChange={setGroupFilter}>
                <SelectTrigger
                  className="ml-auto w-56 bg-background shadow-none"
                  aria-label={t('filterLabel')}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={ALL_GROUPS}>{t('filterAllGroups')}</SelectItem>
                  {groups.map((group) => (
                    <SelectItem key={group.id} value={group.id}>
                      {group.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            ) : null}
          </div>

          {toggleError ? (
            <p
              role="alert"
              className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive"
            >
              {toggleError}
            </p>
          ) : null}

          <ul className="grid gap-3 xl:grid-cols-2">
            {automations.map((automation) => {
              const relative = formatRelativeRun(automation.next_run_at, locale)
              const toggling =
                toggle.isPending && toggle.variables?.id === automation.id
              return (
                <li
                  key={automation.id}
                  className={cn(
                    'group relative overflow-hidden rounded-xl border border-border/70 bg-card p-4 shadow-sm transition-colors hover:border-border',
                    !automation.enabled && 'bg-muted/20',
                  )}
                >
                  {automation.enabled ? (
                    <span
                      aria-hidden
                      className="absolute inset-y-4 left-0 w-0.5 rounded-full bg-primary/70"
                    />
                  ) : null}
                  <div className="flex items-start justify-between gap-3 pl-1.5">
                    <div className="min-w-0">
                      <div className="flex min-w-0 flex-wrap items-center gap-2">
                        <h2 className="truncate text-sm font-semibold tracking-tight">
                          {automation.name}
                        </h2>
                        {!automation.enabled ? (
                          <Badge variant="outline" className="font-normal">
                            {t('card.paused')}
                          </Badge>
                        ) : null}
                      </div>
                      <p className="mt-1 truncate text-xs text-muted-foreground">
                        {groupNames.get(automation.group_id) ?? automation.group_id}
                        <span aria-hidden> · </span>
                        {triggerLabel(automation)}
                      </p>
                    </div>
                    <Switch
                      checked={automation.enabled}
                      disabled={toggling}
                      onCheckedChange={(enabled) => setEnabled(automation, enabled)}
                      aria-label={
                        automation.enabled ? t('card.pause') : t('card.resume')
                      }
                    />
                  </div>

                  <div className="mt-4 flex flex-wrap items-center gap-x-4 gap-y-2 pl-1.5 text-xs text-muted-foreground">
                    <span className="inline-flex min-w-0 items-center gap-1.5">
                      {automation.trigger.kind === 'webhook' ||
                      automation.trigger.kind === 'github' ? (
                        <Repeat2 className="h-3.5 w-3.5 shrink-0" aria-hidden />
                      ) : (
                        <CalendarClock className="h-3.5 w-3.5 shrink-0" aria-hidden />
                      )}
                      {automation.trigger.kind === 'webhook' ||
                      automation.trigger.kind === 'github' ? (
                        <span>{t('card.eventDriven')}</span>
                      ) : automation.next_run_at === null ? (
                        <span>{t('card.noNextRun')}</span>
                      ) : (
                        <span className="truncate">
                          {t('card.nextRun', {
                            when: formatAutomationTime(automation.next_run_at, locale),
                          })}
                          {relative ? (
                            <span className="text-foreground/60"> · {relative}</span>
                          ) : null}
                        </span>
                      )}
                    </span>
                    <span className="inline-flex items-center gap-1.5">
                      <span>{t('card.lastRun')}:</span>
                      {automation.latest?.status ? (
                        <AutomationStatusBadge status={automation.latest.status} />
                      ) : (
                        <span>{t('card.neverRun')}</span>
                      )}
                    </span>
                  </div>

                  {automation.latest?.error ? (
                    <p
                      className="mt-1.5 truncate pl-1.5 text-xs text-destructive"
                      title={automation.latest.error}
                    >
                      {automation.latest.error}
                    </p>
                  ) : null}

                  <div className="mt-3 flex flex-wrap items-center gap-1 pl-1.5">
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="h-8 gap-1.5 px-2 text-xs"
                      onClick={() => setEditor({ automation })}
                    >
                      <Pencil className="h-3.5 w-3.5" aria-hidden />
                      {t('card.edit')}
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="h-8 gap-1.5 px-2 text-xs"
                      onClick={() => setHistoryTarget(automation)}
                    >
                      <History className="h-3.5 w-3.5" aria-hidden />
                      {t('card.history')}
                    </Button>
                    {automation.trigger.kind === 'webhook' ||
                    automation.trigger.kind === 'github' ? (
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        className={cn(
                          'h-8 gap-1.5 px-2 text-xs',
                          !automation.hook_configured && 'text-warning-foreground',
                        )}
                        onClick={() => setSecretTarget(automation)}
                      >
                        <KeyRound className="h-3.5 w-3.5" aria-hidden />
                        {t('card.secret')}
                      </Button>
                    ) : null}
                  </div>
                </li>
              )
            })}
          </ul>
        </div>
      )}

      {editor ? (
        <AutomationFormDialog
          open
          automation={editor.automation}
          groups={groups}
          onOpenChange={(open) => {
            if (!open) setEditor(null)
          }}
          onSaved={(saved) => {
            setEditor(null)
            if (
              (saved.trigger.kind === 'webhook' || saved.trigger.kind === 'github') &&
              !saved.hook_configured
            ) {
              setSecretTarget(saved)
            }
          }}
        />
      ) : null}

      {secretTarget ? (
        <AutomationSecretDialog
          open
          automation={secretTarget}
          onOpenChange={(open) => {
            if (!open) setSecretTarget(null)
          }}
          onChanged={() => {
            void automationsQuery.refetch()
          }}
        />
      ) : null}

      {historyTarget ? (
        <AutomationHistoryDialog
          open
          automation={historyTarget}
          onOpenChange={(open) => {
            if (!open) setHistoryTarget(null)
          }}
        />
      ) : null}
    </DetailShell>
  )
}
