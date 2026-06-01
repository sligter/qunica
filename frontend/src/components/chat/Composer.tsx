import { useCallback, useRef, useState } from 'react'

import { MentionPopover } from '@/components/chat/MentionPopover'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import type { GroupAgentRead } from '@/types/api'

interface ComposerProps {
  onSend: (content: string) => void
  onCancel?: () => void
  isStreaming?: boolean
  hint?: string
  groupAgents?: GroupAgentRead[]
}

export function Composer({ onSend, onCancel, isStreaming, hint, groupAgents = [] }: ComposerProps) {
  const [value, setValue] = useState('')
  const [mentionQuery, setMentionQuery] = useState('')
  const [showMention, setShowMention] = useState(false)
  const [mentionStart, setMentionStart] = useState(-1)
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  const send = () => {
    const trimmed = value.trim()
    if (!trimmed) return
    onSend(trimmed)
    setValue('')
    setShowMention(false)
  }

  const handleChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const newValue = e.target.value
    setValue(newValue)

    const cursorPos = e.target.selectionStart
    const textBeforeCursor = newValue.slice(0, cursorPos)
    const atIdx = textBeforeCursor.lastIndexOf('@')

    if (atIdx >= 0) {
      const beforeAt = atIdx > 0 ? textBeforeCursor[atIdx - 1] : ' '
      if (beforeAt === ' ' || beforeAt === '\n' || atIdx === 0) {
        const query = textBeforeCursor.slice(atIdx + 1)
        if (!query.includes(' ') && !query.includes('\n')) {
          setMentionQuery(query)
          setMentionStart(atIdx)
          setShowMention(true)
          return
        }
      }
    }
    setShowMention(false)
  }

  const handleMentionSelect = useCallback(
    (agent: GroupAgentRead) => {
      const before = value.slice(0, mentionStart)
      const cursorPos = textareaRef.current?.selectionStart ?? value.length
      const after = value.slice(cursorPos)
      const inserted = `@${agent.display_name} `
      const newValue = before + inserted + after
      setValue(newValue)
      setShowMention(false)

      requestAnimationFrame(() => {
        const ta = textareaRef.current
        if (ta) {
          const pos = before.length + inserted.length
          ta.setSelectionRange(pos, pos)
          ta.focus()
        }
      })
    },
    [value, mentionStart],
  )

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (showMention) return
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      send()
    }
  }

  return (
    <div className="border-t border-border bg-background px-4 py-3">
      {hint && <p className="mb-1.5 text-[11px] text-muted-foreground">{hint}</p>}
      <div className="relative flex items-end gap-2">
        <div className="relative flex-1">
          <MentionPopover
            agents={groupAgents}
            query={mentionQuery}
            onSelect={handleMentionSelect}
            onClose={() => setShowMention(false)}
            visible={showMention}
          />
          <Textarea
            ref={textareaRef}
            value={value}
            onChange={handleChange}
            onKeyDown={onKeyDown}
            placeholder="Type a message. Use @ to mention an agent. Enter to send, Shift+Enter for newline."
            rows={2}
            className="resize-none"
          />
        </div>
        {isStreaming && (
          <Button variant="outline" size="sm" onClick={onCancel}>
            Stop all
          </Button>
        )}
        <Button size="sm" onClick={send} disabled={!value.trim()}>
          Send
        </Button>
      </div>
    </div>
  )
}
