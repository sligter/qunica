import { useEffect, useRef, useState } from 'react'
import { Users } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { AgentAvatar } from '@/components/chat/AgentAvatar'
import type { GroupAgentRead } from '@/types/api'

type MentionOption = { key: string; label: string; agent: GroupAgentRead | null }

interface MentionPopoverProps {
  agents: GroupAgentRead[]
  query: string
  onSelect: (name: string) => void
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
  const everyoneLabel = t('composer.mentionEveryone')

  const needle = query.toLowerCase()
  const options: MentionOption[] = agents
    .filter((agent) => agent.display_name.toLowerCase().includes(needle))
    .map((agent) => ({ key: agent.id, label: agent.display_name, agent }))
  if (agents.length > 0 && everyoneLabel.toLowerCase().includes(needle)) {
    options.unshift({ key: 'everyone', label: everyoneLabel, agent: null })
  }

  useEffect(() => {
    setActiveIndex(0)
  }, [query])

  useEffect(() => {
    if (!visible) return

    const handler = (e: KeyboardEvent) => {
      if (!(e.target instanceof Node) || !listRef.current?.parentElement?.contains(e.target)) return
      if (e.key === 'ArrowDown') {
        e.preventDefault()
        setActiveIndex((i) => (i + 1) % Math.max(options.length, 1))
      } else if (e.key === 'ArrowUp') {
        e.preventDefault()
        setActiveIndex((i) => (i - 1 + options.length) % Math.max(options.length, 1))
      } else if (
        (e.key === 'Enter' || e.key === 'Tab' || e.key === ' ') &&
        options[activeIndex]
      ) {
        e.preventDefault()
        e.stopPropagation()
        onSelect(options[activeIndex].label)
      } else if (e.key === 'Escape') {
        e.preventDefault()
        onClose()
      }
    }

    window.addEventListener('keydown', handler, true)
    return () => window.removeEventListener('keydown', handler, true)
  }, [visible, options, activeIndex, onSelect, onClose])

  useEffect(() => {
    const el = listRef.current?.children[activeIndex] as HTMLElement | undefined
    el?.scrollIntoView({ block: 'nearest' })
  }, [activeIndex])

  if (!visible || options.length === 0) return null

  return (
    <div
      ref={listRef}
      className="absolute bottom-full left-0 mb-1 max-h-48 w-64 overflow-y-auto rounded-md border border-border bg-background shadow-lg z-50"
      role="listbox"
      aria-label={t('workspace.mentionPicker')}
    >
      {options.map((option, idx) => (
        <button
          key={option.key}
          type="button"
          role="option"
          aria-selected={idx === activeIndex}
          className={`flex w-full items-center gap-2 px-3 py-2 text-left text-sm transition-colors ${
            idx === activeIndex ? 'bg-accent text-accent-foreground' : 'hover:bg-muted'
          }`}
          onMouseEnter={() => setActiveIndex(idx)}
          onMouseDown={(e) => {
            e.preventDefault()
            onSelect(option.label)
          }}
        >
          {/* Decorative: the row already names the agent, and the avatar's own
              label would otherwise announce it a second time. */}
          <span aria-hidden="true">
            {option.agent ? (
              <AgentAvatar
                name={option.agent.display_name}
                avatarUrl={option.agent.avatar_url}
                size="sm"
              />
            ) : (
              <span className="flex h-6 w-6 items-center justify-center rounded-full bg-muted text-muted-foreground">
                <Users className="h-3.5 w-3.5" />
              </span>
            )}
          </span>
          <span className="truncate font-medium">{option.label}</span>
        </button>
      ))}
    </div>
  )
}
