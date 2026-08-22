/**
 * History of everything the Assistant proposed, and what became of each one.
 *
 * The inline cards in a conversation scroll away; this is the durable record.
 * Pending rows stay actionable here so a change proposed in a chat the user has
 * since moved on from is not stranded.
 */

import { useState } from 'react'
import { Trash2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { AssistantApprovalCard } from '@/components/assistant/AssistantApprovalCard'
import { DetailShell } from '@/components/layout/DetailShell'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { PageState } from '@/components/ui/page-state'
import {
  useAppActions,
  useClearAppActions,
  useDeleteAppAction,
} from '@/hooks/useAppActions'
import { cn } from '@/lib/utils'
import type { AppActionRead, AppActionStatus } from '@/types/api'

/** The reason recorded on a failed apply, if the payload carries one. */
function failureReason(action: AppActionRead): string | null {
  if (!action.result_json) return null
  try {
    const parsed = JSON.parse(action.result_json) as { error?: unknown }
    return typeof parsed.error === 'string' ? parsed.error : null
  } catch {
    return null
  }
}

function statusClasses(status: AppActionStatus): string {
  switch (status) {
    case 'applied':
      return 'border-primary/30 bg-primary/10 text-primary'
    case 'failed':
      return 'border-destructive/30 bg-destructive/10 text-destructive'
    case 'pending':
      return 'border-warning bg-warning text-warning-foreground'
    default:
      return 'border-border bg-muted text-muted-foreground'
  }
}

const APP_ACTIONS_PAGE_SIZE = 50

export function AppActionsPage() {
  const { t } = useTranslation('assistant')
  const [page, setPage] = useState(0)
  const [deleteTarget, setDeleteTarget] = useState<AppActionRead | null>(null)
  const [clearOpen, setClearOpen] = useState(false)
  const actions = useAppActions({ limit: APP_ACTIONS_PAGE_SIZE, skip: page * APP_ACTIONS_PAGE_SIZE })
  const deleteAction = useDeleteAppAction()
  const clearActions = useClearAppActions()
  const items = actions.data?.items ?? []

  return (
    <DetailShell
      title={t('actions.title')}
      subtitle={t('actions.description')}
      actions={
        !actions.isLoading && !actions.error && items.length > 0 ? (
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="text-destructive hover:text-destructive"
            disabled={clearActions.isPending}
            onClick={() => setClearOpen(true)}
          >
            <Trash2 className="h-4 w-4" aria-hidden />
            {t('actions.clear')}
          </Button>
        ) : null
      }
    >
      {actions.isLoading ? (
        <PageState inset className="px-0" variant="loading" title={t('setup.loading')} />
      ) : actions.error ? (
        <PageState inset className="px-0" variant="error" title={String(actions.error)} />
      ) : (
        <div className="space-y-4">
          {items.length === 0 ? (
            <p className="text-sm text-muted-foreground">{t('actions.empty')}</p>
          ) : (
            <ul className="flex flex-col gap-3">
              {items.map((item) => {
                const reason = failureReason(item)
                return (
                  <li
                    key={item.id}
                    className="rounded-lg border border-border/70 bg-card p-3 text-sm"
                  >
                    <div className="flex flex-wrap items-start justify-between gap-2">
                      <div className="min-w-0">
                        <p className="leading-6">{item.summary}</p>
                        <p className="mt-0.5 text-xs text-muted-foreground">
                          {item.target_kind} · {item.action} · {item.created_at}
                        </p>
                      </div>
                      <div className="flex shrink-0 items-center gap-1">
                        <span
                          className={cn(
                            'rounded-full border px-2 py-0.5 text-2xs font-medium',
                            statusClasses(item.status),
                          )}
                        >
                          {t(`actions.${item.status}`)}
                        </span>
                        {item.status !== 'pending' && item.status !== 'approved' ? (
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            className="h-7 w-7 text-muted-foreground hover:text-destructive"
                            disabled={deleteAction.isPending || clearActions.isPending}
                            onClick={() => setDeleteTarget(item)}
                            aria-label={t('actions.deleteEntry')}
                            title={t('actions.delete')}
                          >
                            <Trash2 className="h-3.5 w-3.5" aria-hidden />
                          </Button>
                        ) : null}
                      </div>
                    </div>

                    {reason ? (
                      <p className="mt-2 text-xs text-destructive">{reason}</p>
                    ) : null}

                    {item.status === 'pending' ? (
                      <div className="mt-3">
                        <AssistantApprovalCard
                          hideSummary
                          action={{
                            action_id: item.id,
                            target_kind: item.target_kind,
                            action: item.action,
                            summary: item.summary,
                          }}
                        />
                      </div>
                    ) : null}
                  </li>
                )
              })}
            </ul>
          )}

          {page > 0 || actions.data?.has_more ? (
            <nav className="flex shrink-0 items-center justify-center gap-3 border-t border-border pt-3" aria-label={t('actions.pagination')}>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={page === 0 || actions.isFetching}
                onClick={() => setPage((value) => value - 1)}
              >
                {t('actions.previous')}
              </Button>
              <span className="text-xs text-muted-foreground">{t('actions.page', { page: page + 1 })}</span>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={!actions.data?.has_more || actions.isFetching}
                onClick={() => setPage((value) => value + 1)}
              >
                {t('actions.next')}
              </Button>
            </nav>
          ) : null}
        </div>
      )}

      <ConfirmDialog
        open={deleteTarget !== null}
        onOpenChange={(open) => {
          if (!open) setDeleteTarget(null)
        }}
        title={t('actions.deleteTitle')}
        description={t('actions.deleteDescription')}
        confirmLabel={t('actions.delete')}
        destructive
        onConfirm={() => deleteAction.mutateAsync(deleteTarget?.id ?? '')}
      />

      <ConfirmDialog
        open={clearOpen}
        onOpenChange={setClearOpen}
        title={t('actions.clearTitle')}
        description={t('actions.clearDescription')}
        confirmLabel={t('actions.clear')}
        destructive
        onConfirm={async () => {
          await clearActions.mutateAsync()
          setPage(0)
        }}
      />
    </DetailShell>
  )
}
