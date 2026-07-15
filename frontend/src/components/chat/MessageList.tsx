import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { HumanInputRequestForm } from '@/components/chat/HumanInputRequestForm'
import { MessageItem } from '@/components/chat/MessageItem'
import { StreamTimeline } from '@/components/chat/StreamTimeline'
import { TurnSummary } from '@/components/chat/TurnSummary'
import { humanInputRequestFromText } from '@/lib/humanInput'
import { useMessageStore, type StreamRun } from '@/stores/messageStore'
import type { Message } from '@/types/api'

interface MessageListProps {
  groupId: string
  hasOlderMessages?: boolean
  isLoadingOlderMessages?: boolean
  onLoadOlderMessages?: () => void
  onSubmitHumanInput?: (content: string) => void
  onViewTurnTrace?: (turnId: string, trigger: HTMLButtonElement) => void
}

const EMPTY_MESSAGES: readonly Message[] = []
const EMPTY_WARNINGS: readonly string[] = []
const EMPTY_STREAM_RUNS: Record<string, never> = {}
const EMPTY_STREAM_RUN_IDS: Record<string, never> = {}
const BOTTOM_PROXIMITY_PX = 120
const MESSAGE_SCROLL_KEY_PREFIX = 'ag-swarmer:groups:message-scroll:'

function scrollStorageKey(groupId: string): string {
  return `${MESSAGE_SCROLL_KEY_PREFIX}${groupId}`
}

function readStoredScrollTop(groupId: string): number | null {
  const value = sessionStorage.getItem(scrollStorageKey(groupId))
  if (value === null) return null
  const parsed = Number(value)
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null
}

function storeScrollTop(groupId: string, value: number): void {
  sessionStorage.setItem(scrollStorageKey(groupId), String(Math.max(0, Math.round(value))))
}

function maxScrollTop(node: HTMLDivElement): number {
  return Math.max(0, node.scrollHeight - node.clientHeight)
}

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
  onViewTurnTrace,
}: MessageListProps) {
  const messages = useMessageStore((s) => s.byGroup[groupId] ?? EMPTY_MESSAGES)
  const warnings = useMessageStore(
    (s) => s.warningsByGroup[groupId] ?? EMPTY_WARNINGS,
  )
  const streamRuns = useMessageStore(
    (s) => s.streamRunsByGroup[groupId] ?? EMPTY_STREAM_RUNS,
  )
  const streamRunIdsByUserMessageId = useMessageStore(
    (s) => s.streamRunIdByUserMessageIdByGroup[groupId] ?? EMPTY_STREAM_RUN_IDS,
  )
  const scrollRef = useRef<HTMLDivElement | null>(null)
  const endRef = useRef<HTMLDivElement | null>(null)
  const isNearBottomRef = useRef(true)
  const restoredScrollRef = useRef(false)
  const [showJumpToLatest, setShowJumpToLatest] = useState(false)

  const hiddenMessageIds = useMemo(() => timelineMessageIds(streamRuns), [streamRuns])
  const latestWarning = warnings[warnings.length - 1]
  const warningInputRequest = humanInputRequestFromText(latestWarning)
  const hasActiveStreamRun = useMemo(
    () => Object.values(streamRuns).some((run) => run.status === 'active'),
    [streamRuns],
  )

  const getScrollState = useCallback(() => {
    const node = scrollRef.current
    if (!node) return { canScroll: false, isNearBottom: true }
    const distance = node.scrollHeight - node.scrollTop - node.clientHeight
    const canScroll = node.scrollHeight - node.clientHeight > 1
    return {
      canScroll,
      isNearBottom: !canScroll || distance < BOTTOM_PROXIMITY_PX,
    }
  }, [])

  const updateNearBottom = useCallback(() => {
    const node = scrollRef.current
    if (node) storeScrollTop(groupId, node.scrollTop)
    const { canScroll, isNearBottom } = getScrollState()
    isNearBottomRef.current = isNearBottom
    setShowJumpToLatest(hasActiveStreamRun && canScroll && !isNearBottom)
  }, [getScrollState, groupId, hasActiveStreamRun])

  const jumpToLatest = () => {
    endRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' })
    const node = scrollRef.current
    if (node) storeScrollTop(groupId, maxScrollTop(node))
    isNearBottomRef.current = true
    setShowJumpToLatest(false)
  }

  useEffect(() => {
    restoredScrollRef.current = false
    isNearBottomRef.current = true
    setShowJumpToLatest(false)
  }, [groupId])

  useEffect(() => {
    if (restoredScrollRef.current) return
    if (messages.length === 0 && Object.keys(streamRuns).length === 0) return
    const node = scrollRef.current
    if (!node) return
    const storedScrollTop = readStoredScrollTop(groupId)
    if (storedScrollTop === null) return

    const maxRestorableScrollTop = maxScrollTop(node)
    node.scrollTop = Math.min(storedScrollTop, maxRestorableScrollTop)
    const { canScroll, isNearBottom } = getScrollState()
    isNearBottomRef.current = isNearBottom
    setShowJumpToLatest(hasActiveStreamRun && canScroll && !isNearBottom)
    restoredScrollRef.current = true
  }, [getScrollState, groupId, hasActiveStreamRun, messages.length, streamRuns])

  useEffect(() => {
    const { canScroll, isNearBottom } = getScrollState()
    const shouldStickToBottom = isNearBottomRef.current || isNearBottom
    if (shouldStickToBottom) {
      endRef.current?.scrollIntoView({
        behavior: hasActiveStreamRun ? 'auto' : 'smooth',
        block: 'end',
      })
      const node = scrollRef.current
      if (node) storeScrollTop(groupId, maxScrollTop(node))
      isNearBottomRef.current = true
      setShowJumpToLatest(false)
      return
    }
    isNearBottomRef.current = false
    setShowJumpToLatest(hasActiveStreamRun && canScroll)
  }, [messages, streamRuns, warnings, groupId, hasActiveStreamRun, getScrollState])

  return (
    <div
      ref={scrollRef}
      className="relative flex flex-1 flex-col overflow-y-auto py-4"
      onScroll={updateNearBottom}
    >
      <div className="mx-auto flex w-full max-w-3xl flex-1 flex-col">
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
          const runId = streamRunIdsByUserMessageId[m.id] ?? m.id
          const run = m.sender_type === 'user' ? streamRuns[runId] : undefined
          const turnId = m.sender_type === 'user' ? (run?.turn_id ?? m.turn_id) : null
          const schedulerStatus = run?.scheduler_status ?? m.turn_summary?.status ?? null
          return (
            <Fragment key={m.id}>
              <MessageItem
                message={m}
                groupId={groupId}
                onSubmitHumanInput={onSubmitHumanInput}
              />
              {turnId && schedulerStatus && onViewTurnTrace ? (
                <TurnSummary
                  turnId={turnId}
                  status={schedulerStatus}
                  summaries={run?.criticalSummaries}
                  onViewTrace={onViewTurnTrace}
                />
              ) : null}
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
              <div className="text-center text-xs text-warning-foreground">{latestWarning}</div>
            )}
          </div>
        )}
      </div>
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
