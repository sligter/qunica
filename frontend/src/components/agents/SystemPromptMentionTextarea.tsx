import * as React from 'react'
import { Bot, Users } from 'lucide-react'

import { Textarea } from '@/components/ui/textarea'
import { useAgents } from '@/hooks/useAgents'
import { useGroups } from '@/hooks/useGroups'
import { cn } from '@/lib/utils'
import type { AgentRead, GroupRead } from '@/types/api'

const NO_DESCRIPTION = 'No description provided'

interface ActiveMention {
  start: number
  end: number
  query: string
}

type MentionSuggestion =
  | {
      type: 'agent'
      id: string
      name: string
      description: string | null
      source: AgentRead
    }
  | {
      type: 'team'
      id: string
      name: string
      description: string | null
      announcement: string | null
      source: GroupRead
    }

export interface SystemPromptMentionTextareaProps
  extends Omit<React.TextareaHTMLAttributes<HTMLTextAreaElement>, 'onChange' | 'value'> {
  value: string
  onChange: (value: string) => void
  inputRef?: React.Ref<HTMLTextAreaElement>
}

function getActiveMention(value: string, cursorPosition: number): ActiveMention | null {
  const beforeCursor = value.slice(0, cursorPosition)
  const match = /(^|\s)@([^\s@]*)$/.exec(beforeCursor)

  if (!match) return null

  const leadingLength = match[1]?.length ?? 0
  const query = match[2] ?? ''
  const start = match.index + leadingLength

  return {
    start,
    end: cursorPosition,
    query,
  }
}

function normalizeDescription(description: string | null): string {
  const trimmed = description?.trim()
  return trimmed && trimmed.length > 0 ? trimmed : NO_DESCRIPTION
}

function formatSuggestionContext(suggestion: MentionSuggestion): string {
  if (suggestion.type === 'agent') {
    return `[Agent: ${suggestion.name}]\nDescription: ${normalizeDescription(suggestion.description)}`
  }

  const lines = [
    `[Team: ${suggestion.name}]`,
    `Description: ${normalizeDescription(suggestion.description)}`,
  ]
  const announcement = suggestion.announcement?.trim()

  if (announcement) {
    lines.push(`Announcement: ${announcement}`)
  }

  return lines.join('\n')
}

function assignTextareaRef(
  ref: React.Ref<HTMLTextAreaElement> | undefined,
  node: HTMLTextAreaElement | null,
) {
  if (!ref) return

  if (typeof ref === 'function') {
    ref(node)
    return
  }

  ref.current = node
}

export function SystemPromptMentionTextarea({
  value,
  onChange,
  inputRef,
  onBlur,
  onKeyDown,
  onSelect,
  className,
  ...props
}: SystemPromptMentionTextareaProps) {
  const agents = useAgents()
  const groups = useGroups()
  const textareaRef = React.useRef<HTMLTextAreaElement | null>(null)
  const [activeMention, setActiveMention] = React.useState<ActiveMention | null>(null)
  const [activeIndex, setActiveIndex] = React.useState(0)
  const listboxId = React.useId()

  const setTextareaRef = React.useCallback(
    (node: HTMLTextAreaElement | null) => {
      textareaRef.current = node
      assignTextareaRef(inputRef, node)
    },
    [inputRef],
  )

  const suggestions = React.useMemo<MentionSuggestion[]>(() => {
    const query = activeMention?.query.trim().toLowerCase() ?? ''
    const agentSuggestions = (agents.data ?? []).map((agent) => ({
      type: 'agent' as const,
      id: agent.id,
      name: agent.name,
      description: agent.description,
      source: agent,
    }))
    const teamSuggestions = (groups.data ?? []).map((group) => ({
      type: 'team' as const,
      id: group.id,
      name: group.name,
      description: group.description,
      announcement: group.announcement,
      source: group,
    }))

    return [...agentSuggestions, ...teamSuggestions]
      .filter((suggestion) => suggestion.name.toLowerCase().includes(query))
      .slice(0, 12)
  }, [activeMention?.query, agents.data, groups.data])

  const isPopoverVisible = activeMention !== null && suggestions.length > 0

  React.useEffect(() => {
    setActiveIndex(0)
  }, [activeMention?.query])

  React.useEffect(() => {
    if (activeIndex >= suggestions.length) {
      setActiveIndex(0)
    }
  }, [activeIndex, suggestions.length])

  const updateActiveMention = React.useCallback((nextValue: string, cursorPosition: number) => {
    setActiveMention(getActiveMention(nextValue, cursorPosition))
  }, [])

  const handleChange = (event: React.ChangeEvent<HTMLTextAreaElement>) => {
    const nextValue = event.currentTarget.value
    onChange(nextValue)
    updateActiveMention(nextValue, event.currentTarget.selectionStart)
  }

  const handleSelectSuggestion = React.useCallback(
    (suggestion: MentionSuggestion) => {
      if (!activeMention) return

      const insertText = formatSuggestionContext(suggestion)
      const nextValue = `${value.slice(0, activeMention.start)}${insertText}${value.slice(
        activeMention.end,
      )}`
      const nextCursorPosition = activeMention.start + insertText.length

      onChange(nextValue)
      setActiveMention(null)

      window.requestAnimationFrame(() => {
        textareaRef.current?.focus()
        textareaRef.current?.setSelectionRange(nextCursorPosition, nextCursorPosition)
      })
    },
    [activeMention, onChange, value],
  )

  const handleKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (isPopoverVisible) {
      if (event.key === 'ArrowDown') {
        event.preventDefault()
        setActiveIndex((index) => (index + 1) % suggestions.length)
        return
      }

      if (event.key === 'ArrowUp') {
        event.preventDefault()
        setActiveIndex((index) => (index - 1 + suggestions.length) % suggestions.length)
        return
      }

      if (event.key === 'Enter' || event.key === 'Tab') {
        event.preventDefault()
        handleSelectSuggestion(suggestions[activeIndex])
        return
      }

      if (event.key === 'Escape') {
        event.preventDefault()
        setActiveMention(null)
        return
      }
    }

    onKeyDown?.(event)
  }

  const handleSelect = (event: React.SyntheticEvent<HTMLTextAreaElement>) => {
    onSelect?.(event)
    const textarea = event.currentTarget
    updateActiveMention(value, textarea.selectionStart)
  }

  const handleBlur = (event: React.FocusEvent<HTMLTextAreaElement>) => {
    onBlur?.(event)
    setActiveMention(null)
  }

  return (
    <div className="relative">
      <Textarea
        {...props}
        ref={setTextareaRef}
        value={value}
        onChange={handleChange}
        onKeyDown={handleKeyDown}
        onSelect={handleSelect}
        onBlur={handleBlur}
        aria-autocomplete="list"
        aria-controls={isPopoverVisible ? listboxId : undefined}
        aria-expanded={isPopoverVisible}
        className={className}
      />
      {isPopoverVisible ? (
        <div
          id={listboxId}
          role="listbox"
          className="absolute left-0 top-full z-50 mt-1 max-h-64 w-full overflow-y-auto rounded-md border border-border bg-background shadow-lg"
        >
          {suggestions.map((suggestion, index) => {
            const isActive = index === activeIndex
            const Icon = suggestion.type === 'agent' ? Bot : Users
            const typeLabel = suggestion.type === 'agent' ? 'Agent' : 'Team'

            return (
              <button
                key={`${suggestion.type}-${suggestion.id}`}
                type="button"
                role="option"
                aria-selected={isActive}
                className={cn(
                  'flex w-full items-start gap-2 px-3 py-2 text-left text-sm transition-colors',
                  isActive ? 'bg-accent text-accent-foreground' : 'hover:bg-muted',
                )}
                onMouseEnter={() => setActiveIndex(index)}
                onMouseDown={(event) => {
                  event.preventDefault()
                  handleSelectSuggestion(suggestion)
                }}
              >
                <span className="mt-0.5 rounded-md border border-border bg-muted p-1 text-muted-foreground">
                  <Icon className="h-3.5 w-3.5" aria-hidden="true" />
                </span>
                <span className="min-w-0 flex-1">
                  <span className="flex items-center gap-2">
                    <span className="truncate font-medium">{suggestion.name}</span>
                    <span className="shrink-0 rounded-sm bg-muted px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide text-muted-foreground">
                      {typeLabel}
                    </span>
                  </span>
                  <span className="block truncate text-xs text-muted-foreground">
                    {normalizeDescription(suggestion.description)}
                  </span>
                </span>
              </button>
            )
          })}
        </div>
      ) : null}
    </div>
  )
}
