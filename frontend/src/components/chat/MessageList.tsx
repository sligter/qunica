import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ArrowRight, ChevronDown, Sparkles } from 'lucide-react'

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
  moderatorEnabled?: boolean
}

const EMPTY_MESSAGES: readonly Message[] = []
const EMPTY_WARNINGS: readonly string[] = []
const EMPTY_STREAM_RUNS: Record<string, never> = {}
const EMPTY_STREAM_RUN_IDS: Record<string, never> = {}
const BOTTOM_PROXIMITY_PX = 120
const MESSAGE_SCROLL_KEY_PREFIX = 'qunica:groups:message-scroll:'
const ASSISTANT_SUGGESTION_KEYS = [
  'messages.assistantSuggestions.inspect',
  'messages.assistantSuggestions.createAgent',
  'messages.assistantSuggestions.template',
] as const

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

function timelineMessageIds(
  runs: Record<string, StreamRun>,
  runIdsByUserMessageId: Record<string, string>,
  skippedRunIds: Set<string>,
): Set<string> {
  const ids = new Set<string>()
  for (const runId of new Set(Object.values(runIdsByUserMessageId))) {
    if (skippedRunIds.has(runId)) continue
    const run = runs[runId]
    if (!run) continue
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

function checkpointedRunIds(
  messages: readonly Message[],
  runs: Record<string, StreamRun>,
  runIdsByUserMessageId: Record<string, string>,
): Set<string> {
  const drafts = new Map<string, string>()
  for (const [runId, run] of Object.entries(runs)) {
    if (run.status === 'active') continue
    for (const event of run.events) {
      if (event.type !== 'response_draft' || event.status !== 'streaming' || event.message_id) {
        continue
      }
      const key = `${runId}:${event.agent_id}`
      drafts.set(key, (drafts.get(key) ?? '') + event.content)
    }
  }

  const checkpointed = new Set<string>()
  let latestUserMessageId: string | undefined
  for (const message of messages) {
    if (message.sender_type === 'user') {
      latestUserMessageId = message.id
      continue
    }
    if (message.sender_type !== 'agent') continue
    const linkedRunId = message.reply_to_message_id
      ? runIdsByUserMessageId[message.reply_to_message_id]
      : undefined
    const runId = linkedRunId ?? (
      latestUserMessageId ? runIdsByUserMessageId[latestUserMessageId] : undefined
    )
    if (!runId) continue
    const draft = drafts.get(`${runId}:${message.sender_id}`)
    if (draft && (message.content ?? '').startsWith(draft)) checkpointed.add(runId)
  }
  return checkpointed
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
  moderatorEnabled,
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

  const checkpointedRuns = useMemo(
    () => checkpointedRunIds(messages, streamRuns, streamRunIdsByUserMessageId),
    [messages, streamRuns, streamRunIdsByUserMessageId],
  )
  const hiddenMessageIds = useMemo(
    () => timelineMessageIds(streamRuns, streamRunIdsByUserMessageId, checkpointedRuns),
    [checkpointedRuns, streamRuns, streamRunIdsByUserMessageId],
  )
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
          agentIsSystem ? (
            <div className="flex flex-1 flex-col items-center justify-center px-5 py-8 text-center">
              <span className="flex h-12 w-12 items-center justify-center rounded-2xl bg-primary/10 text-primary shadow-xs">
                <Sparkles className="h-5 w-5" aria-hidden />
              </span>
              <h2 className="mt-4 font-serif text-lg font-semibold tracking-tight">
                {t('messages.assistantEmpty')}
              </h2>
              <p className="mt-1.5 max-w-sm text-xs leading-5 text-muted-foreground">
                {t('messages.assistantEmptyHint')}
              </p>
              {onSubmitHumanInput ? (
                <div className="mt-5 grid w-full max-w-sm gap-2">
                  {ASSISTANT_SUGGESTION_KEYS.map((key) => {
                    const suggestion = t(key)
                    return (
                      <button
                        key={key}
                        type="button"
                        onClick={() => onSubmitHumanInput(suggestion)}
                        className="group flex w-full items-center justify-between gap-3 rounded-xl border border-border/80 bg-card px-3 py-2.5 text-left text-xs leading-5 text-foreground shadow-xs transition-[border-color,background-color,transform] hover:-translate-y-px hover:border-primary/35 hover:bg-primary/5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                      >
                        <span>{suggestion}</span>
                        <ArrowRight className="h-3.5 w-3.5 shrink-0 text-muted-foreground transition-[color,transform] group-hover:translate-x-0.5 group-hover:text-primary" aria-hidden />
                      </button>
                    )
                  })}
                </div>
              ) : null}
            </div>
          ) : (
            <div className="flex flex-1 items-center justify-center px-8 text-center text-sm text-muted-foreground">
              {t('messages.empty')} {t('messages.emptyHint')}
            </div>
          )
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
                  agentName={(agentId) =>
                    agents?.find((agent) => agent.agent_id === agentId)?.display_name ?? agentId}
                  onViewTrace={onViewTurnTrace}
                />
              ) : null}
              {run && !checkpointedRuns.has(run.id) ? (
                <StreamTimeline
                  run={run}
                  groupId={groupId}
                  agents={agents}
                  agentIsSystem={agentIsSystem}
                  moderatorEnabled={moderatorEnabled}
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
