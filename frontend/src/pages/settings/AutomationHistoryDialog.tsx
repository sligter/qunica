import { useTranslation } from 'react-i18next'

import { AutomationStatusBadge } from '@/components/automations/AutomationStatusBadge'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { PageState } from '@/components/ui/page-state'
import { useAutomationHistory } from '@/hooks/useAutomations'
import { formatAutomationTime } from '@/lib/automationTime'
import type { AutomationRead } from '@/types/api'

export function AutomationHistoryDialog({
  automation,
  open,
  onOpenChange,
}: {
  automation: AutomationRead
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const { t, i18n } = useTranslation('automations')
  const locale = i18n.resolvedLanguage ?? 'en-US'
  const history = useAutomationHistory(automation.id, open)
  const items = history.data?.items ?? []

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t('history.title')}</DialogTitle>
          <DialogDescription>
            {automation.name} · {t('history.description')}
          </DialogDescription>
        </DialogHeader>

        <div className="max-h-[60vh] overflow-y-auto">
          {history.isLoading ? (
            <PageState
              inset
              variant="loading"
              title={t('history.loading')}
              className="px-0"
            />
          ) : history.error ? (
            <PageState
              inset
              variant="error"
              title={t('errors.load', { message: String(history.error) })}
              className="px-0"
            />
          ) : items.length === 0 ? (
            <PageState
              inset
              variant="empty"
              title={t('history.empty')}
              className="px-0"
            />
          ) : (
            <ul className="divide-y divide-border/70">
              {items.map((item) => (
                <li key={item.id} className="py-3">
                  <div className="flex flex-wrap items-center gap-2">
                    <Badge variant="outline" className="font-normal">
                      {t(`source.${item.source}`)}
                    </Badge>
                    <AutomationStatusBadge status={item.status} />
                    <span className="ml-auto font-mono text-2xs text-muted-foreground">
                      {t('history.version', { version: item.version })}
                    </span>
                  </div>
                  <div className="mt-1.5 grid gap-1 text-xs text-muted-foreground sm:grid-cols-2">
                    {item.scheduled_at !== null ? (
                      <p>
                        {t('history.scheduled')}:{' '}
                        <span className="tabular-nums">
                          {formatAutomationTime(item.scheduled_at, locale)}
                        </span>
                      </p>
                    ) : null}
                    <p>
                      {t('history.created')}:{' '}
                      <span className="tabular-nums">
                        {formatAutomationTime(item.created_at, locale)}
                      </span>
                    </p>
                    {item.job_id ? (
                      <p className="truncate font-mono">
                        {t('history.job')}: {item.job_id.slice(0, 8)}…
                      </p>
                    ) : null}
                    {item.thread_id ? (
                      <p className="truncate font-mono">
                        {t('history.thread')}: {item.thread_id.slice(0, 8)}…
                      </p>
                    ) : null}
                  </div>
                  {item.error ? (
                    <p className="mt-1.5 break-words text-xs text-destructive">
                      {t('history.error')}: {item.error}
                    </p>
                  ) : null}
                </li>
              ))}
            </ul>
          )}
        </div>

        <DialogFooter>
          <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
            {t('history.close')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
