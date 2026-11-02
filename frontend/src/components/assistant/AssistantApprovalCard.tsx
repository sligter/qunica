/**
 * The approve/reject card for one change the Assistant staged.
 *
 * Rendered inline after the Assistant's reply, so the user can decide without
 * opening tool activity or visiting the separate action history.
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { AlertTriangle, Check, ExternalLink, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { Button } from '@/components/ui/button'
import { MobileAction } from '@/components/chat/MobileAction'
import { useAppAction, useResolveAppAction } from '@/hooks/useAppActions'
import { useSystemSettings } from '@/hooks/useSystemSettings'
import { ApiError } from '@/lib/api-v2/client'
import { isPrefill, queryKeysForKind, type PendingAppAction } from '@/lib/appActions'
import type { AppActionStatus } from '@/types/api'
import { assistantPrefillLink } from '@/components/assistant/assistantPrefill'
import { isDesktopRuntime, openLibraryWindow } from '@/lib/desktop'

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
  const queryClient = useQueryClient()
  const systemSettings = useSystemSettings()
  const [error, setError] = useState<string | null>(null)
  const autoApproveStarted = useRef(false)
  const targetRefreshStarted = useRef(false)
  const actionId = isPrefill(action) ? null : action.action_id
  const targetKind = isPrefill(action) ? undefined : action.target_kind
  // Note: absence from a paginated history is not evidence of pending approval.
  const appAction = useAppAction(actionId)
  const savedStatus = appAction.isError ? undefined : appAction.data?.status
  const pending = savedStatus === 'pending'
  let failureReason: string | null = null
  if (savedStatus === 'failed' && appAction.data?.result_json) {
    try {
      const result: unknown = JSON.parse(appAction.data.result_json)
      if (result && typeof result === 'object' && 'error' in result && typeof result.error === 'string') {
        failureReason = result.error
      }
    } catch { /* Older results may not contain a structured error. */ }
  }
  const displayError = failureReason ?? error ?? appAction.error?.message

  const decide = useCallback(
    async (decision: 'approve' | 'reject') => {
      if (!actionId || !pending) return
      setError(null)
      try {
        await resolve.mutateAsync({
          actionId,
          decision,
          targetKind,
        })
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
    },
    [actionId, pending, resolve, targetKind],
  )

  useEffect(() => {
    if (!actionId) return
    if (savedStatus && savedStatus !== 'pending') {
      setError(null)
      if (savedStatus === 'applied' && targetKind && !targetRefreshStarted.current) {
        targetRefreshStarted.current = true
        for (const key of queryKeysForKind(targetKind)) {
          void queryClient.invalidateQueries({ queryKey: key })
        }
      }
      return
    }
    if (
      systemSettings.isError ||
      systemSettings.data?.assistant_auto_approve !== true ||
      !appAction.isFetched ||
      savedStatus !== 'pending' ||
      error ||
      autoApproveStarted.current
    ) {
      return
    }
    autoApproveStarted.current = true
    void decide('approve')
  }, [actionId, appAction.isFetched, decide, error, queryClient, savedStatus, systemSettings.isError, systemSettings.data?.assistant_auto_approve, targetKind])

  if (isPrefill(action)) {
    const prefill = assistantPrefillLink(action.route, action.fields)
    const fields = Object.entries(prefill.fields)
    return (
      <div className="min-w-0 rounded-md border border-border bg-muted/40 p-3 text-sm">
        <p className="text-xs leading-5 text-muted-foreground">{t(prefill.prefilled ? 'actions.prefillHint' : 'actions.manualFormHint')}</p>
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
          <Link to={prefill.route} onClick={(event) => {
            if (!isDesktopRuntime()) return
            event.preventDefault()
            void openLibraryWindow(prefill.route).catch((cause: unknown) => {
              setError(cause instanceof Error ? cause.message : String(cause))
            })
          }}>
            <ExternalLink className="mr-1.5 h-3.5 w-3.5" aria-hidden />
            {t('actions.openLink')}
          </Link>
        </Button>
        {error ? <p role="alert" className="mt-2 text-xs text-destructive">{error}</p> : null}
      </div>
    )
  }

  const statusLabel: Partial<Record<AppActionStatus, string>> = {
    pending: t('actions.pending'),
    expired: t('actions.expired'),
    applied: t('actions.applied'),
    rejected: t('actions.rejected'),
    failed: t('actions.failed'),
    approved: t('actions.running'),
  }

  return (
    <MobileAction active={pending}>
    <div className="min-w-0 rounded-md border border-warning bg-warning/40 p-3 text-sm text-foreground">
      <div className="text-xs font-semibold text-warning-foreground">
        {appAction.data === null ? t('actions.unavailable')
          : appAction.isError ? t('actions.statusError')
          : savedStatus ? statusLabel[savedStatus] ?? savedStatus : t('actions.loadingStatus')}
      </div>
      {hideSummary ? null : <p className="mt-1 leading-6">{action.summary}</p>}

      {!pending ? null : (
        <>
          <p className="mt-1 text-xs text-muted-foreground">{t('actions.nothingChangedYet')}</p>
          <div className="mt-3 flex gap-2">
            <Button
              size="sm"
              disabled={resolve.isPending}
              onClick={() => void decide('approve')}
            >
              <Check className="mr-1.5 h-3.5 w-3.5" aria-hidden />
              {resolve.isPending
                ? systemSettings.data?.assistant_auto_approve === true
                  ? t('actions.autoApproving')
                  : t('actions.approving')
                : t('actions.approve')}
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

      {displayError ? (
        <p className="mt-2 flex items-start gap-1.5 text-xs text-destructive">
          <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden />
          <span className="min-w-0">{displayError}</span>
        </p>
      ) : null}
    </div>
    </MobileAction>
  )
}
