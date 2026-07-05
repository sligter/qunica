import { useEffect, useRef, useState } from 'react'
import { Bot } from 'lucide-react'

import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import type { GroupAgentRead } from '@/types/api'

interface MentionPopoverProps {
  agents: GroupAgentRead[]
  query: string
  onSelect: (agent: GroupAgentRead) => void
  onClose: () => void
  visible: boolean
}

export function MentionPopover({
  agents,
  query,
  onSelect,
  onClose,
  visible,
}: MentionPopoverProps) {
  const [activeIndex, setActiveIndex] = useState(0)
  const listRef = useRef<HTMLDivElement>(null)

  const filtered = agents.filter((a) =>
    a.display_name.toLowerCase().includes(query.toLowerCase()),
  )

  useEffect(() => {
    setActiveIndex(0)
  }, [query])

  useEffect(() => {
    if (!visible) return

    const handler = (e: KeyboardEvent) => {
      if (e.key === 'ArrowDown') {
        e.preventDefault()
        setActiveIndex((i) => (i + 1) % Math.max(filtered.length, 1))
      } else if (e.key === 'ArrowUp') {
        e.preventDefault()
        setActiveIndex((i) => (i - 1 + filtered.length) % Math.max(filtered.length, 1))
      } else if (e.key === 'Enter' && filtered[activeIndex]) {
        e.preventDefault()
        onSelect(filtered[activeIndex])
      } else if (e.key === 'Escape') {
        e.preventDefault()
        onClose()
      }
    }

    window.addEventListener('keydown', handler, true)
    return () => window.removeEventListener('keydown', handler, true)
  }, [visible, filtered, activeIndex, onSelect, onClose])

  useEffect(() => {
    const el = listRef.current?.children[activeIndex] as HTMLElement | undefined
    el?.scrollIntoView({ block: 'nearest' })
  }, [activeIndex])

  if (!visible || filtered.length === 0) return null

  return (
    <div
      ref={listRef}
      className="absolute bottom-full left-0 mb-1 max-h-48 w-64 overflow-y-auto rounded-md border border-border bg-background shadow-lg z-50"
    >
      {filtered.map((agent, idx) => (
        <button
          key={agent.id}
          type="button"
          className={`flex w-full items-center gap-2 px-3 py-2 text-left text-sm transition-colors ${
            idx === activeIndex ? 'bg-accent text-accent-foreground' : 'hover:bg-muted'
          }`}
          onMouseEnter={() => setActiveIndex(idx)}
          onMouseDown={(e) => {
            e.preventDefault()
            onSelect(agent)
          }}
        >
          <Avatar className="h-6 w-6 shrink-0">
            <AvatarFallback className="bg-avatar-5 text-avatar-foreground text-[10px]">
              <Bot className="h-3.5 w-3.5" />
            </AvatarFallback>
          </Avatar>
          <span className="truncate font-medium">{agent.display_name}</span>
        </button>
      ))}
    </div>
  )
}
