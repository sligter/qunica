import { CheckCircle2, SendHorizontal } from 'lucide-react'
import { useState, type FormEvent } from 'react'

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
  const [value, setValue] = useState('')
  const [submitted, setSubmitted] = useState(false)
  const trimmed = value.trim()
  const canSubmit = Boolean(trimmed) && Boolean(onSubmitResponse) && !submitted

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!canSubmit) return
    onSubmitResponse?.(formatHumanInputResponse(trimmed, targetDisplayName))
    setValue('')
    setSubmitted(true)
  }

  return (
    <form
      className={cn(
        'min-w-0 rounded-md border border-amber-200 bg-amber-50/55 p-3 text-foreground',
        compact && 'p-2.5',
        className,
      )}
      onSubmit={submit}
    >
      <div className="flex min-w-0 items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="text-xs font-semibold text-amber-800">Input requested</div>
          <div className="mt-1 text-sm leading-6 text-foreground">
            <MarkdownMessage content={request.question} />
          </div>
        </div>
        {request.required === false ? (
          <span className="shrink-0 rounded-full border border-amber-200 bg-background px-2 py-0.5 text-[10px] font-medium text-amber-700">
            Optional
          </span>
        ) : null}
      </div>

      <div className="mt-3 flex flex-col gap-2 sm:flex-row sm:items-end">
        <Textarea
          value={value}
          onChange={(event) => setValue(event.target.value)}
          placeholder="Type your response..."
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
    </form>
  )
}
