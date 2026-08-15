import { AlertTriangle, Check, ShieldCheck, X } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import type { StreamApprovalRequest } from '@/stores/messageStore'
import { cn } from '@/lib/utils'

export interface ApprovalAnswer {
  tool_call_id: string
  approved: boolean
  remember: boolean
  note?: string
}

interface ToolApprovalCardProps {
  request: StreamApprovalRequest
  /** Set once answered, which turns the card into a record of the decision. */
  resolved?: 'approved' | 'declined'
  onAnswer?: (answer: ApprovalAnswer) => void
  className?: string
}

/**
 * The card a paused tool call is answered from.
 *
 * It shows the exact command that will run, not a paraphrase: the point of the
 * pause is that the user authorises *this* text, and the runtime replays the
 * same call rather than asking the model again. A note is offered on the decline
 * path because "no, because …" is far more useful to the agent than a bare
 * refusal it will otherwise try to work around.
 */
export function ToolApprovalCard({
  request,
  resolved,
  onAnswer,
  className,
}: ToolApprovalCardProps) {
  const { t } = useTranslation('chat')
  const [note, setNote] = useState('')
  const [busy, setBusy] = useState(false)
  const answered = Boolean(resolved)
  const disabled = answered || busy || !onAnswer

  const answer = (approved: boolean, remember: boolean) => {
    if (disabled) return
    setBusy(true)
    onAnswer?.({
      tool_call_id: request.tool_call_id,
      approved,
      remember,
      note: note.trim() || undefined,
    })
  }

  return (
    <div
      className={cn(
        'min-w-0 rounded-md border p-3 text-foreground',
        answered ? 'border-border bg-muted/40' : 'border-warning bg-warning/55',
        className,
      )}
    >
      <div className="flex min-w-0 items-start gap-2">
        {answered ? (
          <ShieldCheck className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
        ) : (
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-warning-foreground" />
        )}
        <div className="min-w-0 flex-1">
          <div
            className={cn(
              'text-xs font-semibold',
              answered ? 'text-muted-foreground' : 'text-warning-foreground',
            )}
          >
            {answered
              ? t(
                  resolved === 'approved'
                    ? 'approval.answeredApproved'
                    : 'approval.answeredDeclined',
                )
              : t('approval.title', { capability: request.capability })}
          </div>

          <pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap break-words rounded bg-background/70 px-2 py-1.5 font-mono text-xs leading-5">
            {request.subject}
          </pre>
          <div className="mt-1.5 text-xs leading-5 text-muted-foreground">
            {t('approval.viaTool', { tool: request.tool_name })} · {request.reason}
          </div>

          {answered ? null : (
            <>
              <Textarea
                aria-label={t('approval.noteLabel')}
                className="mt-2 min-h-[2.25rem] text-xs"
                placeholder={t('approval.notePlaceholder')}
                rows={2}
                value={note}
                onChange={(event) => setNote(event.target.value)}
                disabled={busy}
              />
              <div className="mt-2 flex flex-wrap items-center gap-2">
                <Button
                  size="sm"
                  className="h-7 gap-1.5"
                  disabled={disabled}
                  onClick={() => answer(true, false)}
                >
                  <Check className="h-3 w-3" />
                  {t('approval.allowOnce')}
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  className="h-7 gap-1.5"
                  disabled={disabled}
                  onClick={() => answer(true, true)}
                >
                  <ShieldCheck className="h-3 w-3" />
                  {t('approval.allowForThread', { capability: request.capability })}
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  className="h-7 gap-1.5"
                  disabled={disabled}
                  onClick={() => answer(false, false)}
                >
                  <X className="h-3 w-3" />
                  {t('approval.decline')}
                </Button>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  )
}
