import { useState } from 'react'
import type { KeyboardEvent } from 'react'

import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'

interface ComposerProps {
  isStreaming: boolean
  onSend: (content: string) => void
  onCancel: () => void
  hint?: string
}

export function Composer({ isStreaming, onSend, onCancel, hint }: ComposerProps) {
  const [value, setValue] = useState('')

  const submit = () => {
    const trimmed = value.trim()
    if (!trimmed || isStreaming) return
    onSend(trimmed)
    setValue('')
  }

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      submit()
    }
  }

  return (
    <div className="border-t border-border bg-background p-3">
      <div className="flex flex-col gap-2">
        <Textarea
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder="Type a message. Use @AgentName to address an agent. Enter to send, Shift+Enter for newline."
          rows={2}
          className="resize-none"
          disabled={isStreaming}
        />
        <div className="flex items-center justify-between">
          <p className="text-xs text-muted-foreground">{hint}</p>
          <div className="flex gap-2">
            {isStreaming && (
              <Button variant="outline" size="sm" onClick={onCancel}>
                Cancel
              </Button>
            )}
            <Button size="sm" onClick={submit} disabled={isStreaming || !value.trim()}>
              {isStreaming ? 'Streaming…' : 'Send'}
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}
