import { Fragment, useEffect, useMemo, useRef, useState } from 'react'

import { HumanInputRequestForm } from '@/components/chat/HumanInputRequestForm'
import { MessageItem } from '@/components/chat/MessageItem'
import { StreamTimeline } from '@/components/chat/StreamTimeline'
import { humanInputRequestFromText } from '@/lib/humanInput'
import { useMessageStore, type StreamRun } from '@/stores/messageStore'
import type { Message } from '@/types/api'

interface MessageListProps {
  groupId: string
  hasOlderMessages?: boolean
  isLoadingOlderMessages?: boolean
  onLoadOlderMessages?: () => void
  onSubmitHumanInput?: (content: string) => void
}

const EMPTY_MESSAGES: readonly Message[] = []
const EMPTY_WARNINGS: readonly string[] = []
const EMPTY_STREAM_RUNS: Record<string, never> = {}

function timelineMessageIds(runs: Record<string, StreamRun>): Set<string> {
  const ids = new Set<string>()
  for (const run of Object.values(runs)) {
    for (const event of run.events) {
      if (event.type === 'response_draft' && event.message_id) {
        ids.add(event.message_id)
      }
      if (event.type === 'agent_message') {
        ids.add(event.message_id)
      }
    }
  }
  return ids
}

export function MessageList({
  groupId,
  hasOlderMessages = false,
  isLoadingOlderMessages = false,
  onLoadOlderMessages,
  onSubmitHumanInput,
}: MessageListProps) {
  const messages = useMessageStore((s) => s.byGroup[groupId] ?? EMPTY_MESSAGES)
  const warnings = useMessageStore(
    (s) => s.warningsByGroup[groupId] ?? EMPTY_WARNINGS,
  )
  const streamRuns = useMessageStore(
    (s) => s.streamRunsByGroup[groupId] ?? EMPTY_STREAM_RUNS,
  )
  const scrollRef = useRef<HTMLDivElement | null>(null)
  const endRef = useRef<HTMLDivElement | null>(null)
  const isNearBottomRef = useRef(true)
  const [showJumpToLatest, setShowJumpToLatest] = useState(false)

  const hiddenMessageIds = useMemo(() => timelineMessageIds(streamRuns), [streamRuns])
  const latestWarning = warnings[warnings.length - 1]
  const warningInputRequest = humanInputRequestFromText(latestWarning)
  const hasActiveStreamRun = useMemo(
    () => Object.values(streamRuns).some((run) => run.status === 'active'),
    [streamRuns],
  )

  const updateNearBottom = () => {
    const node = scrollRef.current
    if (!node) return
    const distance = node.scrollHeight - node.scrollTop - node.clientHeight
    const isNearBottom = distance < 120
    isNearBottomRef.current = isNearBottom
    if (isNearBottom) setShowJumpToLatest(false)
  }

  const jumpToLatest = () => {
    endRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' })
    isNearBottomRef.current = true
    setShowJumpToLatest(false)
  }

  useEffect(() => {
    if (isNearBottomRef.current) {
      endRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' })
      return
    }
    if (hasActiveStreamRun) {
      setShowJumpToLatest(true)
    }
  }, [messages, streamRuns, warnings, hasActiveStreamRun])

  return (
    <div
      ref={scrollRef}
      className="relative flex flex-1 flex-col overflow-y-auto py-4"
      onScroll={updateNearBottom}
    >
      {messages.length === 0 && Object.keys(streamRuns).length === 0 && (
        <div className="flex flex-1 items-center justify-center px-8 text-center text-sm text-muted-foreground">
          No messages yet. Try sending <code>@AgentName hello</code> to start.
        </div>
      )}
      {hasOlderMessages && (
        <div className="flex justify-center px-4 pb-3">
          <button
            type="button"
            className="rounded-full border border-border bg-background px-3 py-1 text-xs text-muted-foreground hover:bg-muted hover:text-foreground disabled:cursor-not-allowed disabled:opacity-60"
            disabled={isLoadingOlderMessages}
            onClick={onLoadOlderMessages}
          >
            {isLoadingOlderMessages ? 'Loading earlier messages...' : 'Load earlier messages'}
          </button>
        </div>
      )}
      {messages.map((m) => {
        if (hiddenMessageIds.has(m.id)) return null
        const run = m.sender_type === 'user' ? streamRuns[m.id] : undefined
        return (
          <Fragment key={m.id}>
            <MessageItem
              message={m}
              groupId={groupId}
              onSubmitHumanInput={onSubmitHumanInput}
            />
            {run ? (
              <StreamTimeline run={run} onSubmitHumanInput={onSubmitHumanInput} />
            ) : null}
          </Fragment>
        )
      })}
      {latestWarning && Object.keys(streamRuns).length === 0 && (
        <div className="mt-2 px-4">
          {warningInputRequest ? (
            <div className="mx-auto max-w-2xl">
              <HumanInputRequestForm
                request={warningInputRequest}
                onSubmitResponse={onSubmitHumanInput}
                compact
              />
            </div>
          ) : (
            <div className="text-center text-xs text-amber-600">{latestWarning}</div>
          )}
        </div>
      )}
      {showJumpToLatest && (
        <div className="sticky bottom-3 z-10 flex justify-center">
          <button
            type="button"
            className="rounded-full border border-border bg-background px-3 py-1.5 text-xs font-medium text-foreground shadow-sm hover:bg-muted"
            onClick={jumpToLatest}
          >
            Jump to latest
          </button>
        </div>
      )}
      <div ref={endRef} />
    </div>
  )
}
