import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { AgentAvatar } from '@/components/chat/AgentAvatar'
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
  const { t } = useTranslation('chat')
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
      if (!(e.target instanceof Node) || !listRef.current?.parentElement?.contains(e.target)) return
      if (e.key === 'ArrowDown') {
        e.preventDefault()
        setActiveIndex((i) => (i + 1) % Math.max(filtered.length, 1))
      } else if (e.key === 'ArrowUp') {
        e.preventDefault()
        setActiveIndex((i) => (i - 1 + filtered.length) % Math.max(filtered.length, 1))
      } else if (
        (e.key === 'Enter' || e.key === 'Tab' || e.key === ' ') &&
        filtered[activeIndex]
      ) {
        e.preventDefault()
        e.stopPropagation()
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
      role="listbox"
      aria-label={t('workspace.mentionPicker')}
    >
      {filtered.map((agent, idx) => (
        <button
          key={agent.id}
          type="button"
          role="option"
          aria-selected={idx === activeIndex}
          className={`flex w-full items-center gap-2 px-3 py-2 text-left text-sm transition-colors ${
            idx === activeIndex ? 'bg-accent text-accent-foreground' : 'hover:bg-muted'
          }`}
          onMouseEnter={() => setActiveIndex(idx)}
          onMouseDown={(e) => {
            e.preventDefault()
            onSelect(agent)
          }}
        >
          {/* Decorative: the row already names the agent, and the avatar's own
              label would otherwise announce it a second time. */}
          <span aria-hidden="true">
            <AgentAvatar name={agent.display_name} avatarUrl={agent.avatar_url} size="sm" />
          </span>
          <span className="truncate font-medium">{agent.display_name}</span>
        </button>
      ))}
    </div>
  )
}
