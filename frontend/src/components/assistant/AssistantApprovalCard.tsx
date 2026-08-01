/**
 * The approve/reject card for one change the Assistant staged.
 *
 * Rendered inline in the stream where the tool call happened, so the decision
 * sits next to the reasoning that produced it rather than in a separate queue
 * the user has to go find.
 */

import { useState } from 'react'
import { AlertTriangle, Check, ExternalLink, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { Button } from '@/components/ui/button'
import { useResolveAppAction } from '@/hooks/useAppActions'
import { ApiError } from '@/lib/api-v2/client'
import { isPrefill, type PendingAppAction } from '@/lib/appActions'
import type { AppActionStatus } from '@/types/api'

interface AssistantApprovalCardProps {
  action: PendingAppAction
  /**
   * Hide the summary line. Set where the surrounding row already shows it —
   * repeating it reads as two separate changes.
   */
  hideSummary?: boolean
}

export function AssistantApprovalCard({ action, hideSummary }: AssistantApprovalCardProps) {
  const { t } = useTranslation('assistant')
  const resolve = useResolveAppAction()
  const [resolved, setResolved] = useState<AppActionStatus | null>(null)
  const [error, setError] = useState<string | null>(null)

  if (isPrefill(action)) {
    const fields = Object.entries(action.fields)
    return (
      <div className="min-w-0 rounded-md border border-border bg-muted/40 p-3 text-sm">
        <p className="text-xs leading-5 text-muted-foreground">{t('actions.prefillHint')}</p>
        {fields.length > 0 ? (
          <dl className="mt-2 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
            {fields.map(([key, value]) => (
              <div key={key} className="contents">
                <dt className="text-muted-foreground">{key}</dt>
                <dd className="min-w-0 truncate font-mono">{String(value)}</dd>
              </div>
            ))}
          </dl>
        ) : null}
        <Button asChild size="sm" variant="outline" className="mt-3">
          <Link to={action.route}>
            <ExternalLink className="mr-1.5 h-3.5 w-3.5" aria-hidden />
            {t('actions.openLink')}
          </Link>
        </Button>
      </div>
    )
  }

  const decide = async (decision: 'approve' | 'reject') => {
    setError(null)
    try {
      const result = await resolve.mutateAsync({
        actionId: action.action_id,
        decision,
        targetKind: action.target_kind,
      })
      setResolved(result.status)
    } catch (cause) {
      // The apply can fail on validation the core still performs — a workspace
      // deleted since the proposal, a name now taken. Saying so is the whole
      // point; a silent no-op reads as success.
      setError(
        cause instanceof ApiError
          ? cause.message
          : cause instanceof Error
            ? cause.message
            : String(cause),
      )
    }
  }

  const statusLabel: Partial<Record<AppActionStatus, string>> = {
    applied: t('actions.applied'),
    rejected: t('actions.rejected'),
    failed: t('actions.failed'),
    approved: t('actions.approved'),
  }

  return (
    <div className="min-w-0 rounded-md border border-warning bg-warning/40 p-3 text-sm text-foreground">
      <div className="text-xs font-semibold text-warning-foreground">
        {t('actions.pending')}
      </div>
      {hideSummary ? null : <p className="mt-1 leading-6">{action.summary}</p>}

      {resolved ? (
        <p className="mt-2 text-xs font-medium">{statusLabel[resolved] ?? resolved}</p>
      ) : (
        <>
          <p className="mt-1 text-xs text-muted-foreground">{t('actions.nothingChangedYet')}</p>
          <div className="mt-3 flex gap-2">
            <Button
              size="sm"
              disabled={resolve.isPending}
              onClick={() => void decide('approve')}
            >
              <Check className="mr-1.5 h-3.5 w-3.5" aria-hidden />
              {resolve.isPending ? t('actions.approving') : t('actions.approve')}
            </Button>
            <Button
              size="sm"
              variant="outline"
              disabled={resolve.isPending}
              onClick={() => void decide('reject')}
            >
              <X className="mr-1.5 h-3.5 w-3.5" aria-hidden />
              {t('actions.reject')}
            </Button>
          </div>
        </>
      )}

      {error ? (
        <p className="mt-2 flex items-start gap-1.5 text-xs text-destructive">
          <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden />
          <span className="min-w-0">{error}</span>
        </p>
      ) : null}
    </div>
  )
}
