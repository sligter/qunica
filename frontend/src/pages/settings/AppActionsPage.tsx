/**
 * History of everything the Assistant proposed, and what became of each one.
 *
 * The inline cards in a conversation scroll away; this is the durable record.
 * Pending rows stay actionable here so a change proposed in a chat the user has
 * since moved on from is not stranded.
 */

import { useEffect, useState } from 'react'
import { Search, Trash2, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { AssistantApprovalCard } from '@/components/assistant/AssistantApprovalCard'
import { DetailShell } from '@/components/layout/DetailShell'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
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
const ALL_STATUS = '__all__'
const STATUS_FILTERS: AppActionStatus[] = [
  'pending',
  'approved',
  'applied',
  'rejected',
  'failed',
  'expired',
]

export function AppActionsPage() {
  const { t } = useTranslation('assistant')
  const [page, setPage] = useState(0)
  const [deleteTarget, setDeleteTarget] = useState<AppActionRead | null>(null)
  const [clearOpen, setClearOpen] = useState(false)
  // The text field holds every keystroke; the query only follows debounced
  // state, so typing does not fire a request per character.
  const [searchInput, setSearchInput] = useState('')
  const [search, setSearch] = useState('')
  const [status, setStatus] = useState<AppActionStatus | typeof ALL_STATUS>(ALL_STATUS)

  useEffect(() => {
    const timer = window.setTimeout(() => {
      const trimmed = searchInput.trim()
      setSearch((current) => (trimmed === current ? current : trimmed))
      setPage(0)
    }, 300)
    return () => window.clearTimeout(timer)
  }, [searchInput])

  const actions = useAppActions({
    limit: APP_ACTIONS_PAGE_SIZE,
    skip: page * APP_ACTIONS_PAGE_SIZE,
    q: search || undefined,
    status: status === ALL_STATUS ? undefined : status,
  })
  const deleteAction = useDeleteAppAction()
  const clearActions = useClearAppActions()
  const items = actions.data?.items ?? []
  // A page that predates the backend `total` (or a failed count) still pages by
  // has_more; the label simply omits the size.
  const total = actions.data?.total
  const pageCount = total !== undefined && total > 0 ? Math.ceil(total / APP_ACTIONS_PAGE_SIZE) : undefined
  const filtersActive = Boolean(search) || status !== ALL_STATUS

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
          <div className="flex flex-wrap items-center gap-3">
            <div className="relative min-w-0 max-w-xs flex-1">
              <Search
                className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground"
                aria-hidden
              />
              <Input
                type="search"
                value={searchInput}
                onChange={(event) => setSearchInput(event.target.value)}
                placeholder={t('actions.searchPlaceholder')}
                aria-label={t('actions.search')}
                className="pl-9"
              />
              {searchInput ? (
                <button
                  type="button"
                  className="absolute right-2.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                  aria-label={t('common:actions.clear')}
                  onClick={() => setSearchInput('')}
                >
                  <X className="h-4 w-4" aria-hidden />
                </button>
              ) : null}
            </div>
            <Select
              value={status}
              onValueChange={(next) => {
                setStatus(next === ALL_STATUS ? ALL_STATUS : (next as AppActionStatus))
                setPage(0)
              }}
            >
              <SelectTrigger className="w-40 bg-background shadow-none" aria-label={t('actions.statusFilter')}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ALL_STATUS}>{t('actions.allStatuses')}</SelectItem>
                {STATUS_FILTERS.map((option) => (
                  <SelectItem key={option} value={option}>
                    {t(`actions.${option}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {filtersActive ? (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => {
                  setSearchInput('')
                  setStatus(ALL_STATUS)
                  setPage(0)
                }}
              >
                {t('actions.clearFilters')}
              </Button>
            ) : null}
            {total !== undefined ? (
              <span className="ml-auto text-xs text-muted-foreground">
                {t('actions.total', {
                  count: total,
                  formattedCount: total,
                })}
              </span>
            ) : null}
          </div>

          {items.length === 0 && !filtersActive ? (
            <p className="text-sm text-muted-foreground">{t('actions.empty')}</p>
          ) : items.length === 0 ? (
            <p className="text-sm text-muted-foreground">{t('actions.noMatches')}</p>
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
              <span className="text-xs text-muted-foreground">
                {pageCount !== undefined
                  ? t('actions.pageOf', { page: page + 1, count: pageCount })
                  : t('actions.page', { page: page + 1 })}
              </span>
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
