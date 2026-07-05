import { CheckCircle2, SendHorizontal } from 'lucide-react'
import { useId, useState, type FormEvent } from 'react'

import { MarkdownMessage } from '@/components/chat/MarkdownMessage'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { formatHumanInputResponse, type HumanInputRequest } from '@/lib/humanInput'
import { cn } from '@/lib/utils'

interface HumanInputRequestFormProps {
  request: HumanInputRequest
  onSubmitResponse?: (content: string) => void
  targetDisplayName?: string
  className?: string
  compact?: boolean
}

export function HumanInputRequestForm({
  request,
  onSubmitResponse,
  targetDisplayName,
  className,
  compact = false,
}: HumanInputRequestFormProps) {
  const inputId = useId()
  const [value, setValue] = useState('')
  const [selectedChoice, setSelectedChoice] = useState('')
  const [submitted, setSubmitted] = useState(false)
  const trimmed = value.trim()
  const choices = request.choices ?? []
  const hasChoices = choices.length > 0 || request.input_type === 'choice'
  const answer = selectedChoice
    ? [selectedChoice, trimmed].filter(Boolean).join('\n\n')
    : trimmed
  const canSubmit = Boolean(answer) && Boolean(onSubmitResponse) && !submitted

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!canSubmit) return
    onSubmitResponse?.(formatHumanInputResponse(answer, targetDisplayName))
    setValue('')
    setSelectedChoice('')
    setSubmitted(true)
  }

  return (
    <form
      className={cn(
        'min-w-0 rounded-md border border-warning bg-warning/55 p-3 text-foreground',
        compact && 'p-2.5',
        className,
      )}
      onSubmit={submit}
    >
      <div className="flex min-w-0 items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="text-xs font-semibold text-warning-foreground">Input requested</div>
          <div className="mt-1 text-sm leading-6 text-foreground">
            <MarkdownMessage content={request.question} />
          </div>
        </div>
        {request.required === false ? (
          <span className="shrink-0 rounded-full border border-warning bg-background px-2 py-0.5 text-[10px] font-medium text-warning-foreground">
            Optional
          </span>
        ) : null}
      </div>

      <div className="mt-3 flex flex-col gap-3">
        {choices.length > 0 ? (
          <div className="grid gap-1.5">
            <div className="text-xs font-medium text-muted-foreground">Choose an option</div>
            <div className="flex flex-wrap gap-1.5">
              {choices.map((choice) => {
                const selected = choice === selectedChoice
                return (
                  <button
                    key={choice}
                    type="button"
                    aria-pressed={selected}
                    disabled={submitted || !onSubmitResponse}
                    className={cn(
                      'rounded-md border px-2.5 py-1.5 text-xs font-medium transition-colors',
                      selected
                        ? 'border-warning-foreground/60 bg-warning text-warning-foreground'
                        : 'border-warning bg-background text-foreground hover:bg-warning/70',
                      'disabled:cursor-not-allowed disabled:opacity-60',
                    )}
                    onClick={() => setSelectedChoice(selected ? '' : choice)}
                  >
                    {choice}
                  </button>
                )
              })}
            </div>
          </div>
        ) : null}
        <div className="flex flex-col gap-2 sm:flex-row sm:items-end">
          <label className="sr-only" htmlFor={inputId}>
            {hasChoices ? 'Additional details' : 'Response'}
          </label>
          <Textarea
            id={inputId}
            value={value}
            onChange={(event) => setValue(event.target.value)}
            placeholder={hasChoices ? 'Add details (optional)...' : 'Type your response...'}
            rows={compact ? 2 : 3}
            disabled={submitted || !onSubmitResponse}
            className="min-h-0 resize-none bg-background"
          />
          <Button type="submit" size="sm" className="shrink-0" disabled={!canSubmit}>
            {submitted ? (
              <CheckCircle2 className="h-3.5 w-3.5" />
            ) : (
              <SendHorizontal className="h-3.5 w-3.5" />
            )}
            {submitted ? 'Sent' : 'Submit'}
          </Button>
        </div>
      </div>
    </form>
  )
}
