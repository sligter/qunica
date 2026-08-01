/**
 * History of everything the Assistant proposed, and what became of each one.
 *
 * The inline cards in a conversation scroll away; this is the durable record.
 * Pending rows stay actionable here so a change proposed in a chat the user has
 * since moved on from is not stranded.
 */

import { useTranslation } from 'react-i18next'

import { AssistantApprovalCard } from '@/components/assistant/AssistantApprovalCard'
import { PageState } from '@/components/ui/page-state'
import { SectionHeading } from '@/components/ui/section'
import { useAppActions } from '@/hooks/useAppActions'
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

export function AppActionsPage() {
  const { t } = useTranslation('assistant')
  const actions = useAppActions()

  if (actions.isLoading) return <PageState variant="loading" title={t('setup.loading')} />
  if (actions.error) return <PageState variant="error" title={String(actions.error)} />

  const items = actions.data ?? []

  return (
    <div className="flex h-full flex-col gap-4 overflow-y-auto p-6">
      <SectionHeading title={t('actions.title')} description={t('actions.description')} />

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
                  <span
                    className={cn(
                      'shrink-0 rounded-full border px-2 py-0.5 text-2xs font-medium',
                      statusClasses(item.status),
                    )}
                  >
                    {t(`actions.${item.status}`)}
                  </span>
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
    </div>
  )
}
