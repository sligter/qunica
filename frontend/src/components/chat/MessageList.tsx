import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ChevronDown } from 'lucide-react'

import { HumanInputRequestForm } from '@/components/chat/HumanInputRequestForm'
import { MessageItem } from '@/components/chat/MessageItem'
import { StreamTimeline } from '@/components/chat/StreamTimeline'
import { TurnSummary } from '@/components/chat/TurnSummary'
import { humanInputRequestFromText } from '@/lib/humanInput'
import { useMessageStore, type StreamRun } from '@/stores/messageStore'
import type { Message } from '@/types/api'
import type { GroupAgentRead } from '@/types/api'
import type { ConversationScope } from '@/hooks/useGroupMessages'

interface MessageListProps {
  groupId: string
  stateId?: string
  /** Thread the surrounding view's message query is keyed by, if any. */
  threadId?: string
  hasOlderMessages?: boolean
  isLoadingOlderMessages?: boolean
  onLoadOlderMessages?: () => void
  onSubmitHumanInput?: (content: string) => void
  onViewTurnTrace?: (turnId: string, trigger: HTMLButtonElement) => void
  scope?: ConversationScope
  agents?: GroupAgentRead[]
  agentIsSystem?: boolean
}

const EMPTY_MESSAGES: readonly Message[] = []
const EMPTY_WARNINGS: readonly string[] = []
const EMPTY_STREAM_RUNS: Record<string, never> = {}
const EMPTY_STREAM_RUN_IDS: Record<string, never> = {}
const BOTTOM_PROXIMITY_PX = 120
const MESSAGE_SCROLL_KEY_PREFIX = 'ag-swarmer:groups:message-scroll:'

const warningKeys = {
  'No one replied': 'messages.warnings.noReply',
  'Waiting for your input': 'messages.warnings.waitingForInput',
  'Stream warning': 'messages.warnings.streamWarning',
  'Stream failed': 'messages.warnings.streamFailed',
} as const

function isKnownWarning(warning: string): warning is keyof typeof warningKeys {
  return Object.prototype.hasOwnProperty.call(warningKeys, warning)
}

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
  stateId = groupId,
  threadId,
  hasOlderMessages = false,
  isLoadingOlderMessages = false,
  onLoadOlderMessages,
  onSubmitHumanInput,
  onViewTurnTrace,
  scope = 'groups',
  agents,
  agentIsSystem,
}: MessageListProps) {
  const { t } = useTranslation('chat')
  const messages = useMessageStore((s) => s.byGroup[stateId] ?? EMPTY_MESSAGES)
  const warnings = useMessageStore(
    (s) => s.warningsByGroup[stateId] ?? EMPTY_WARNINGS,
  )
  const streamRuns = useMessageStore(
    (s) => s.streamRunsByGroup[stateId] ?? EMPTY_STREAM_RUNS,
  )
  const streamRunIdsByUserMessageId = useMessageStore(
    (s) => s.streamRunIdByUserMessageIdByGroup[stateId] ?? EMPTY_STREAM_RUN_IDS,
  )
  const scrollRef = useRef<HTMLDivElement | null>(null)
  const endRef = useRef<HTMLDivElement | null>(null)
  const isNearBottomRef = useRef(true)
  const restoredScrollRef = useRef(false)
  const [showJumpToLatest, setShowJumpToLatest] = useState(false)

  const hiddenMessageIds = useMemo(() => timelineMessageIds(streamRuns), [streamRuns])
  const latestWarning = warnings[warnings.length - 1]
  const warningInputRequest = humanInputRequestFromText(latestWarning)
  const warningLabel = latestWarning && isKnownWarning(latestWarning)
    ? t(warningKeys[latestWarning])
    : latestWarning
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
    if (node) storeScrollTop(stateId, node.scrollTop)
    const { canScroll, isNearBottom } = getScrollState()
    isNearBottomRef.current = isNearBottom
    setShowJumpToLatest(canScroll && !isNearBottom)
  }, [getScrollState, stateId])

  const jumpToLatest = () => {
    endRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' })
    const node = scrollRef.current
    if (node) storeScrollTop(stateId, maxScrollTop(node))
    isNearBottomRef.current = true
    setShowJumpToLatest(false)
  }

  useEffect(() => {
    restoredScrollRef.current = false
    isNearBottomRef.current = true
    setShowJumpToLatest(false)
  }, [stateId])

  useEffect(() => {
    if (restoredScrollRef.current) return
    if (messages.length === 0 && Object.keys(streamRuns).length === 0) return
    const node = scrollRef.current
    if (!node) return
    const storedScrollTop = readStoredScrollTop(stateId)

    const maxRestorableScrollTop = maxScrollTop(node)
    node.scrollTop = storedScrollTop === null
      ? maxRestorableScrollTop
      : Math.min(storedScrollTop, maxRestorableScrollTop)
    const { canScroll, isNearBottom } = getScrollState()
    isNearBottomRef.current = isNearBottom
    setShowJumpToLatest(canScroll && !isNearBottom)
    restoredScrollRef.current = true
  }, [getScrollState, hasActiveStreamRun, messages.length, stateId, streamRuns])

  useEffect(() => {
    if (messages.length === 0 && Object.keys(streamRuns).length === 0) return
    const { canScroll, isNearBottom } = getScrollState()
    const shouldStickToBottom = isNearBottomRef.current || isNearBottom
    if (shouldStickToBottom) {
      endRef.current?.scrollIntoView({
        behavior: hasActiveStreamRun ? 'auto' : 'smooth',
        block: 'end',
      })
      const node = scrollRef.current
      if (node) storeScrollTop(stateId, maxScrollTop(node))
      isNearBottomRef.current = true
      setShowJumpToLatest(false)
      return
    }
    isNearBottomRef.current = false
    setShowJumpToLatest(canScroll)
  }, [messages, streamRuns, warnings, stateId, hasActiveStreamRun, getScrollState])

  return (
    <div
      ref={scrollRef}
      className="relative flex min-w-0 flex-1 flex-col overflow-x-hidden overflow-y-auto py-4"
      onScroll={updateNearBottom}
    >
      <div className="mx-auto flex min-w-0 w-full max-w-6xl flex-1 flex-col">
        {messages.length === 0 && Object.keys(streamRuns).length === 0 && (
          <div className="flex flex-1 items-center justify-center px-8 text-center text-sm text-muted-foreground">
            {t('messages.empty')} {t('messages.emptyHint')}
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
              {isLoadingOlderMessages ? t('messages.loadingOlder') : t('messages.loadOlder')}
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
                stateId={stateId}
                threadId={threadId}
                scope={scope}
                agents={agents}
                agentIsSystem={agentIsSystem}
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
                <StreamTimeline
                  run={run}
                  groupId={groupId}
                  agents={agents}
                  agentIsSystem={agentIsSystem}
                  onSubmitHumanInput={onSubmitHumanInput}
                  stateId={stateId}
                  scope={scope}
                />
              ) : null}
            </Fragment>
          )
        })}
        {latestWarning && Object.keys(streamRuns).length === 0 && (
          <div className="mt-2 px-4">
            {warningInputRequest ? (
              <div className="mx-auto w-full max-w-5xl">
                <HumanInputRequestForm
                  request={warningInputRequest}
                  onSubmitResponse={onSubmitHumanInput}
                  compact
                />
              </div>
            ) : (
              <div className="text-center text-xs text-warning-foreground">{warningLabel}</div>
            )}
          </div>
        )}
      </div>
      {showJumpToLatest && (
        <div className="sticky bottom-3 z-10 flex justify-center">
          <button
            type="button"
            className="flex h-10 w-10 items-center justify-center rounded-full border border-border bg-background text-foreground shadow-md hover:bg-muted"
            onClick={jumpToLatest}
            aria-label={t('messages.jumpLatest')}
            title={t('messages.jumpLatest')}
          >
            <ChevronDown className="h-5 w-5" />
          </button>
        </div>
      )}
      <div ref={endRef} />
    </div>
  )
}
