/**
 * Message store — sink for the SSE message stream.
 *
 * `byGroup` is the canonical chronological history per group; populated from
 * `GET /messages` on mount and appended to as `user_message` / `agent_message`
 * events arrive. `inFlightByGroup` holds the partial bubbles while tokens
 * are still streaming, keyed per (groupId, agentId) within a single send
 * batch. When the matching `agent_message` event arrives we drop the
 * in-flight entry and append the persisted Message to `byGroup`.
 */

import { create } from 'zustand'

import type { HumanInputRequest } from '@/lib/humanInput'
import type {
  GroupTurnTraceResponse,
  GroupTurnStatus,
  GroupTurnTerminationReason,
  SchedulerStreamUpdate,
} from '@/lib/api-v2/types'
import type { ContextUsage, Message } from '@/types/api'

export interface StreamingBubble {
  id: string
  agent_id: string
  stream_id: string | null
  content: string
}

export interface ActiveAgent {
  agent_id: string
  display_name: string
  index: number
  total: number
  round?: number
  stream_id?: string | null
  context_usage?: ContextUsage | null
}

export type ToolActivityStatus =
  | 'started'
  | 'completed'
  | 'failed'
  | 'unavailable'
  | 'setup_required'
  | 'workspace_required'
  | 'input_required'
  | 'approval_required'

export interface ToolActivity {
  id: string
  agent_id: string
  display_name: string
  tool_name: string
  status: ToolActivityStatus
  args_summary?: string
  result_summary?: string
  input_request?: HumanInputRequest
}

export type StreamRunStatus = 'active' | 'completed' | 'error' | 'cancelled'

export type SchedulerCriticalSummaryKind =
  | 'deterministic_selection'
  | 'call'
  | 'handoff'
  | 'dispatch_failed'
  | 'moderator_fallback'
  | 'cancelled'
  | 'superseded'
  | 'budget_exhausted'
  | 'waiting_for_user'
  | 'silence'
  | 'failed'

export interface SchedulerCriticalSummary {
  id: string
  kind: SchedulerCriticalSummaryKind
  message: string
  count: number
  source_agent_id?: string | null
  target_agent_id?: string
  created_at: string
}

interface StreamTimelineEventBase {
  id: string
  stream_id: string
  created_at: string
  updated_at?: string
}

export interface StreamAgentStartEvent extends StreamTimelineEventBase {
  type: 'agent_start'
  agent_id: string
  display_name: string
  index?: number
  total?: number
  round?: number
  context_usage?: ContextUsage | null
}

export interface StreamResponseDraftEvent extends StreamTimelineEventBase {
  type: 'response_draft'
  agent_id: string
  display_name: string
  content: string
  status: 'streaming' | 'finalized'
  message_id?: string
  context_usage?: ContextUsage | null
}

export interface StreamReasoningEvent extends StreamTimelineEventBase {
  type: 'reasoning'
  agent_id: string
  display_name: string
  content: string
  status: 'streaming' | 'done'
}

export interface StreamToolEvent extends StreamTimelineEventBase {
  type: 'tool'
  agent_id: string
  display_name: string
  tool_call_id: string
  tool_name: string
  status: ToolActivityStatus
  args_summary?: string
  result_summary?: string
  input_request?: HumanInputRequest
}

export interface StreamExternalRunEvent extends StreamTimelineEventBase {
  type: 'external_run'
  run_id: string
  agent_id: string
  display_name: string
  adapter?: string
  status?: string
  cwd?: string
  exit_code?: number
  summary?: string
}

export interface StreamAgentMessageEvent extends StreamTimelineEventBase {
  type: 'agent_message'
  message_id: string
  agent_id: string
  display_name: string
  content: string
  context_usage?: ContextUsage | null
}

export interface StreamNoticeEvent extends StreamTimelineEventBase {
  type: 'agent_silent' | 'agent_handoff' | 'waiting_for_user' | 'warning' | 'agent_error' | 'done'
  message: string
  agent_id?: string
  display_name?: string
  input_request?: HumanInputRequest
}

export type StreamTimelineEvent =
  | StreamAgentStartEvent
  | StreamResponseDraftEvent
  | StreamReasoningEvent
  | StreamToolEvent
  | StreamExternalRunEvent
  | StreamAgentMessageEvent
  | StreamNoticeEvent

export interface StreamRun {
  id: string
  group_id: string
  user_message_id: string
  status: StreamRunStatus
  turn_id: string | null
  scheduler_status: GroupTurnStatus | null
  terminal_reason: GroupTurnTerminationReason | null
  criticalSummaries: SchedulerCriticalSummary[]
  created_at: string
  updated_at: string
  events: StreamTimelineEvent[]
}

interface MessageState {
  byGroup: Record<string, Message[]>
  inFlightByGroup: Record<string, Record<string, StreamingBubble>>
  activeAgentsByGroup: Record<string, Record<string, ActiveAgent>>
  warningsByGroup: Record<string, string[]>
  toolActivityByGroup: Record<string, ToolActivity[]>
  streamRunsByGroup: Record<string, Record<string, StreamRun>>
  streamRunIdByUserMessageIdByGroup: Record<string, Record<string, string>>
  streamRunOrderByGroup: Record<string, string[]>
  resumingMessageIds: Set<string>

  setHistory: (groupId: string, messages: Message[]) => void
  prependHistory: (groupId: string, messages: Message[]) => void
  clearGroupMessages: (groupId: string) => void
  removeMessage: (groupId: string, messageId: string) => void
  appendMessage: (groupId: string, message: Message) => void
  patchInFlight: (groupId: string, agentId: string, delta: string, streamId?: string | null) => void
  finalizeInFlight: (groupId: string, message: Message) => void
  clearInFlight: (groupId: string) => void
  clearStreamInFlight: (groupId: string, streamId: string) => void
  clearAgentInFlight: (groupId: string, agentId: string, streamId?: string | null) => void
  setActiveAgent: (groupId: string, agent: ActiveAgent) => void
  clearActiveAgent: (groupId: string, agentId?: string, streamId?: string | null) => void
  pushWarning: (groupId: string, warning: string) => void
  clearWarnings: (groupId: string) => void
  pushToolActivity: (groupId: string, activity: ToolActivity) => void
  clearToolActivity: (groupId: string) => void
  startStreamRun: (groupId: string, streamId: string, userMessage: Message) => void
  addStreamAgentStart: (groupId: string, streamId: string, agent: ActiveAgent) => void
  setStreamAgentContextUsage: (
    groupId: string,
    streamId: string,
    agentId: string,
    usage: ContextUsage,
  ) => void
  patchStreamDraft: (
    groupId: string,
    streamId: string,
    agentId: string,
    delta: string,
    displayName?: string,
  ) => void
  patchStreamReasoning: (
    groupId: string,
    streamId: string,
    agentId: string,
    delta: string,
    displayName?: string,
  ) => void
  clearStreamingStreamDraft: (groupId: string, streamId: string, agentId: string) => void
  finalizeStreamDraft: (groupId: string, streamId: string, message: Message, displayName?: string) => void
  upsertStreamTool: (groupId: string, streamId: string, activity: ToolActivity) => void
  upsertStreamExternalRun: (
    groupId: string,
    streamId: string,
    event: Omit<StreamExternalRunEvent, 'id' | 'type' | 'stream_id' | 'created_at' | 'updated_at'>,
  ) => void
  appendStreamNotice: (
    groupId: string,
    streamId: string,
    event: Omit<StreamNoticeEvent, 'id' | 'stream_id' | 'created_at'>,
  ) => void
  applySchedulerEvent: (
    groupId: string,
    streamId: string,
    update: SchedulerStreamUpdate,
  ) => boolean
  linkStreamRunToUserMessage: (
    groupId: string,
    streamId: string,
    userMessageId: string,
  ) => void
  detachStreamRun: (groupId: string, streamId: string) => void
  reconcileSchedulerTurn: (groupId: string, trace: GroupTurnTraceResponse) => void
  acceptsStreamEvent: (groupId: string, streamId: string) => boolean
  markStreamRunWaitingForUser: (groupId: string, streamId: string) => string | null
  markStreamRunDone: (groupId: string, streamId: string) => void
  markStreamRunError: (groupId: string, streamId: string, message: string) => void
  markStreamRunCancelled: (groupId: string, streamIds?: string[]) => void
  appendToMessage: (groupId: string, messageId: string, delta: string) => void
  replaceMessage: (groupId: string, message: Message) => void
  startResume: (messageId: string) => void
  endResume: (messageId: string) => void
}

const MAX_COMPLETED_STREAM_RUNS_PER_GROUP = 12
const MAX_CRITICAL_SUMMARIES_PER_RUN = 20

function inFlightKey(agentId: string, streamId: string | null | undefined): string {
  return `${streamId ?? 'default'}:${agentId}`
}

function nowIso(): string {
  return new Date().toISOString()
}

function emptyStreamRun(groupId: string, streamId: string, timestamp: string): StreamRun {
  return {
    id: streamId,
    group_id: groupId,
    user_message_id: streamId,
    status: 'active',
    turn_id: null,
    scheduler_status: null,
    terminal_reason: null,
    criticalSummaries: [],
    created_at: timestamp,
    updated_at: timestamp,
    events: [],
  }
}

function appendCriticalSummary(
  summaries: SchedulerCriticalSummary[],
  summary: SchedulerCriticalSummary,
): SchedulerCriticalSummary[] {
  const next = [...summaries, summary]
  return next.slice(-MAX_CRITICAL_SUMMARIES_PER_RUN)
}

function schedulerSummary(
  update: SchedulerStreamUpdate,
  timestamp: string,
): SchedulerCriticalSummary | null {
  switch (update.kind) {
    case 'speaker_selected': {
      const { action_kind: actionKind, source_agent_id: sourceId, target_agent_id: targetId } =
        update.payload
      if (actionKind === 'call') {
        return {
          id: update.event_id,
          kind: 'call',
          message: `Agent call routed to ${targetId}`,
          count: 1,
          source_agent_id: sourceId,
          target_agent_id: targetId,
          created_at: timestamp,
        }
      }
      if (actionKind === 'handoff') {
        return {
          id: update.event_id,
          kind: 'handoff',
          message: `Handoff routed to ${targetId}`,
          count: 1,
          source_agent_id: sourceId,
          target_agent_id: targetId,
          created_at: timestamp,
        }
      }
      if (update.payload.reason === 'moderator_fallback') return null
      return {
        id: update.event_id,
        kind: 'deterministic_selection',
        message: 'Scheduler selected 1 speaker',
        count: 1,
        created_at: timestamp,
      }
    }
    case 'dispatch_failed':
      return {
        id: update.event_id,
        kind: 'dispatch_failed',
        message: `Dispatch to ${update.payload.target_agent_id} failed`,
        count: 1,
        target_agent_id: update.payload.target_agent_id,
        created_at: timestamp,
      }
    case 'moderator_fallback':
      return {
        id: update.event_id,
        kind: 'moderator_fallback',
        message: `Moderator fallback selected ${update.payload.target_agent_id}`,
        count: 1,
        target_agent_id: update.payload.target_agent_id,
        created_at: timestamp,
      }
    case 'turn_cancelled':
      return {
        id: update.event_id,
        kind: 'cancelled',
        message: 'Turn cancelled',
        count: 1,
        created_at: timestamp,
      }
    case 'turn_superseded':
      return {
        id: update.event_id,
        kind: 'superseded',
        message: 'Turn superseded by a newer message',
        count: 1,
        created_at: timestamp,
      }
    case 'turn_budget_exhausted':
      return {
        id: update.event_id,
        kind: 'budget_exhausted',
        message:
          update.payload.status === 'failure_budget_exhausted'
            ? 'Turn stopped after repeated failures'
            : 'Turn reached its budget limit',
        count: 1,
        created_at: timestamp,
      }
    case 'turn_completed': {
      switch (update.payload.status) {
        case 'waiting_for_user':
          return {
            id: update.event_id,
            kind: 'waiting_for_user',
            message: 'Turn is waiting for user input',
            count: 1,
            created_at: timestamp,
          }
        case 'silence':
          return {
            id: update.event_id,
            kind: 'silence',
            message: 'Turn completed without a visible reply',
            count: 1,
            created_at: timestamp,
          }
        case 'failed':
          return {
            id: update.event_id,
            kind: 'failed',
            message: 'Turn failed while persisting scheduler state',
            count: 1,
            created_at: timestamp,
          }
        case 'budget_exhausted':
        case 'failure_budget_exhausted':
          return {
            id: update.event_id,
            kind: 'budget_exhausted',
            message:
              update.payload.status === 'failure_budget_exhausted'
                ? 'Turn stopped after repeated failures'
                : 'Turn reached its budget limit',
            count: 1,
            created_at: timestamp,
          }
        case 'completed':
          return null
      }
      return null
    }
    case 'turn_started':
    case 'done':
      return null
  }
}

function appendOrFoldSchedulerSummary(
  summaries: SchedulerCriticalSummary[],
  summary: SchedulerCriticalSummary | null,
): SchedulerCriticalSummary[] {
  if (!summary) return summaries
  const last = summaries[summaries.length - 1]
  if (summary.kind === 'deterministic_selection' && last?.kind === summary.kind) {
    const count = last.count + summary.count
    const next = summaries.slice()
    next[next.length - 1] = {
      ...last,
      id: summary.id,
      count,
      message: `Scheduler selected ${count} speakers`,
    }
    return next
  }
  if (
    last?.kind === summary.kind &&
    (summary.kind === 'cancelled' ||
      summary.kind === 'superseded' ||
      summary.kind === 'budget_exhausted' ||
      summary.kind === 'waiting_for_user' ||
      summary.kind === 'silence' ||
      summary.kind === 'failed')
  ) {
    return summaries
  }
  return appendCriticalSummary(summaries, summary)
}

function schedulerTerminalFields(update: SchedulerStreamUpdate): {
  status: GroupTurnStatus | null
  reason: GroupTurnTerminationReason | null
} {
  switch (update.kind) {
    case 'turn_started':
      return { status: 'running', reason: null }
    case 'turn_cancelled':
    case 'turn_superseded':
    case 'turn_budget_exhausted':
    case 'turn_completed':
      return { status: update.payload.status, reason: update.payload.reason }
    case 'speaker_selected':
    case 'dispatch_failed':
    case 'moderator_fallback':
    case 'done':
      return { status: null, reason: null }
  }
}

function schedulerTurnId(update: SchedulerStreamUpdate): string {
  return update.payload.turn_id
}

function schedulerStatusAcceptsEvents(status: GroupTurnStatus | null): boolean {
  return (
    status === null ||
    status === 'pending' ||
    status === 'running' ||
    status === 'waiting_for_user'
  )
}

function streamStatusFromSchedulerStatus(status: GroupTurnStatus): StreamRunStatus {
  if (status === 'cancelled') return 'cancelled'
  if (status === 'failed') return 'error'
  if (status === 'pending' || status === 'running' || status === 'waiting_for_user') {
    return 'active'
  }
  return 'completed'
}

function reconciledTurnSummary(
  turnId: string,
  status: GroupTurnStatus,
  timestamp: string,
): SchedulerCriticalSummary | null {
  const base = { id: `reconciled:${turnId}:${status}`, count: 1, created_at: timestamp }
  switch (status) {
    case 'waiting_for_user':
      return { ...base, kind: 'waiting_for_user', message: 'Turn is waiting for user input' }
    case 'cancelled':
      return { ...base, kind: 'cancelled', message: 'Turn cancelled' }
    case 'superseded':
      return { ...base, kind: 'superseded', message: 'Turn superseded by a newer message' }
    case 'budget_exhausted':
      return { ...base, kind: 'budget_exhausted', message: 'Turn reached its budget limit' }
    case 'failure_budget_exhausted':
      return {
        ...base,
        kind: 'budget_exhausted',
        message: 'Turn stopped after repeated failures',
      }
    case 'silence':
      return { ...base, kind: 'silence', message: 'Turn completed without a visible reply' }
    case 'failed':
      return { ...base, kind: 'failed', message: 'Turn failed' }
    case 'pending':
    case 'running':
    case 'completed':
      return null
  }
}

function upsertTimelineEvent(
  events: StreamTimelineEvent[],
  event: StreamTimelineEvent,
): StreamTimelineEvent[] {
  const index = events.findIndex((item) => item.id === event.id)
  if (index === -1) return [...events, event]
  return events.map((item, itemIndex) => (itemIndex === index ? event : item))
}

/**
 * Auto-collapse the most recent still-streaming reasoning block for an agent.
 * Called whenever a non-reasoning part (text, tool, notice) arrives for that
 * agent, so the "Thinking" disclosure closes once the model starts answering.
 */
function markReasoningDone(
  events: StreamTimelineEvent[],
  agentId: string,
): StreamTimelineEvent[] {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index]
    if (event.type === 'reasoning' && event.agent_id === agentId) {
      if (event.status === 'streaming') {
        const next = events.slice()
        next[index] = { ...event, status: 'done' }
        return next
      }
      return events
    }
  }
  return events
}

/** Append a model-text delta, opening a new segment after any tool/reasoning break. */
function appendTextSegment(
  events: StreamTimelineEvent[],
  streamId: string,
  agentId: string,
  delta: string,
  displayName: string | undefined,
  timestamp: string,
): StreamTimelineEvent[] {
  const base = markReasoningDone(events, agentId)
  const last = base[base.length - 1]
  if (
    last &&
    last.type === 'response_draft' &&
    last.agent_id === agentId &&
    last.status === 'streaming'
  ) {
    const next = base.slice()
    next[base.length - 1] = {
      ...last,
      display_name: displayName ?? last.display_name,
      content: last.content + delta,
      updated_at: timestamp,
    }
    return next
  }
  const segment: StreamResponseDraftEvent = {
    id: `response:${streamId}:${agentId}:${base.length}`,
    type: 'response_draft',
    stream_id: streamId,
    agent_id: agentId,
    display_name: displayName ?? 'Agent',
    content: delta,
    status: 'streaming',
    created_at: timestamp,
    updated_at: timestamp,
  }
  return [...base, segment]
}

function pruneStreamRuns(
  runs: Record<string, StreamRun>,
  order: string[],
): { runs: Record<string, StreamRun>; order: string[]; removedIds: Set<string> } {
  const completedIds = order.filter((id) => {
    const run = runs[id]
    return run !== undefined && run.status !== 'active'
  })
  const removeCount = Math.max(0, completedIds.length - MAX_COMPLETED_STREAM_RUNS_PER_GROUP)
  if (removeCount === 0) return { runs, order, removedIds: new Set() }
  const removeIds = new Set(completedIds.slice(0, removeCount))
  const nextRuns = Object.fromEntries(
    Object.entries(runs).filter(([id]) => !removeIds.has(id)),
  )
  const nextOrder = order.filter((id) => !removeIds.has(id))
  return { runs: nextRuns, order: nextOrder, removedIds: removeIds }
}

function pruneStreamRunIdMap(
  runIdsByMessage: Record<string, string>,
  removedRunIds: Set<string>,
): Record<string, string> {
  if (removedRunIds.size === 0) return runIdsByMessage
  return Object.fromEntries(
    Object.entries(runIdsByMessage).filter(([, runId]) => !removedRunIds.has(runId)),
  )
}

export const useMessageStore = create<MessageState>((set, get) => ({
  byGroup: {},
  inFlightByGroup: {},
  activeAgentsByGroup: {},
  warningsByGroup: {},
  toolActivityByGroup: {},
  streamRunsByGroup: {},
  streamRunIdByUserMessageIdByGroup: {},
  streamRunOrderByGroup: {},
  resumingMessageIds: new Set(),

  setHistory: (groupId, messages) =>
    set((s) => ({
      byGroup: { ...s.byGroup, [groupId]: messages },
    })),

  prependHistory: (groupId, messages) =>
    set((s) => {
      const existing = s.byGroup[groupId] ?? []
      const existingIds = new Set(existing.map((message) => message.id))
      const older = messages.filter((message) => !existingIds.has(message.id))
      return {
        byGroup: { ...s.byGroup, [groupId]: [...older, ...existing] },
      }
    }),

  clearGroupMessages: (groupId) =>
    set((s) => ({
      byGroup: { ...s.byGroup, [groupId]: [] },
      inFlightByGroup: { ...s.inFlightByGroup, [groupId]: {} },
      activeAgentsByGroup: { ...s.activeAgentsByGroup, [groupId]: {} },
      warningsByGroup: { ...s.warningsByGroup, [groupId]: [] },
      toolActivityByGroup: { ...s.toolActivityByGroup, [groupId]: [] },
      streamRunsByGroup: { ...s.streamRunsByGroup, [groupId]: {} },
      streamRunIdByUserMessageIdByGroup: {
        ...s.streamRunIdByUserMessageIdByGroup,
        [groupId]: {},
      },
      streamRunOrderByGroup: { ...s.streamRunOrderByGroup, [groupId]: [] },
    })),

  removeMessage: (groupId, messageId) =>
    set((s) => {
      const messages = s.byGroup[groupId] ?? []
      const groupRunIdsByMessage = s.streamRunIdByUserMessageIdByGroup[groupId] ?? {}
      const runId = groupRunIdsByMessage[messageId]
      const nextState: Partial<MessageState> = {
        byGroup: {
          ...s.byGroup,
          [groupId]: messages.filter((message) => message.id !== messageId),
        },
      }

      if (!runId) return nextState

      const nextRunIdsByMessage = { ...groupRunIdsByMessage }
      delete nextRunIdsByMessage[messageId]

      const groupRuns = s.streamRunsByGroup[groupId] ?? {}
      const nextRuns = { ...groupRuns }
      delete nextRuns[runId]

      return {
        ...nextState,
        streamRunsByGroup: { ...s.streamRunsByGroup, [groupId]: nextRuns },
        streamRunIdByUserMessageIdByGroup: {
          ...s.streamRunIdByUserMessageIdByGroup,
          [groupId]: nextRunIdsByMessage,
        },
        streamRunOrderByGroup: {
          ...s.streamRunOrderByGroup,
          [groupId]: (s.streamRunOrderByGroup[groupId] ?? []).filter((id) => id !== runId),
        },
      }
    }),

  appendMessage: (groupId, message) =>
    set((s) => ({
      byGroup: {
        ...s.byGroup,
        [groupId]: [...(s.byGroup[groupId] ?? []), message],
      },
    })),

  patchInFlight: (groupId, agentId, delta, streamId = null) =>
    set((s) => {
      const groupInFlight = s.inFlightByGroup[groupId] ?? {}
      const bubbleId = inFlightKey(agentId, streamId)
      const existing = groupInFlight[bubbleId]
      const next: StreamingBubble = existing
        ? { ...existing, content: existing.content + delta }
        : { id: bubbleId, agent_id: agentId, stream_id: streamId, content: delta }
      return {
        inFlightByGroup: {
          ...s.inFlightByGroup,
          [groupId]: { ...groupInFlight, [bubbleId]: next },
        },
      }
    }),

  finalizeInFlight: (groupId, message) =>
    set((s) => {
      const agentId = message.sender_id ?? ''
      const groupInFlight = s.inFlightByGroup[groupId] ?? {}
      const remaining = { ...groupInFlight }
      delete remaining[inFlightKey(agentId, message.reply_to_message_id)]
      delete remaining[inFlightKey(agentId, null)]
      const groupActive = s.activeAgentsByGroup[groupId] ?? {}
      const remainingActive = { ...groupActive }
      delete remainingActive[inFlightKey(agentId, message.reply_to_message_id)]
      delete remainingActive[inFlightKey(agentId, null)]
      return {
        byGroup: {
          ...s.byGroup,
          [groupId]: [...(s.byGroup[groupId] ?? []), message],
        },
        inFlightByGroup: {
          ...s.inFlightByGroup,
          [groupId]: remaining,
        },
        activeAgentsByGroup: {
          ...s.activeAgentsByGroup,
          [groupId]: remainingActive,
        },
      }
    }),

  clearInFlight: (groupId) =>
    set((s) => ({
      inFlightByGroup: { ...s.inFlightByGroup, [groupId]: {} },
      activeAgentsByGroup: { ...s.activeAgentsByGroup, [groupId]: {} },
      toolActivityByGroup: { ...s.toolActivityByGroup, [groupId]: [] },
    })),

  clearStreamInFlight: (groupId, streamId) =>
    set((s) => {
      const streamPrefix = `${streamId}:`
      const groupInFlight = s.inFlightByGroup[groupId] ?? {}
      const remainingInFlight = Object.fromEntries(
        Object.entries(groupInFlight).filter(([key]) => !key.startsWith(streamPrefix)),
      )
      const groupActive = s.activeAgentsByGroup[groupId] ?? {}
      const remainingActive = Object.fromEntries(
        Object.entries(groupActive).filter(([key]) => !key.startsWith(streamPrefix)),
      )
      return {
        inFlightByGroup: { ...s.inFlightByGroup, [groupId]: remainingInFlight },
        activeAgentsByGroup: { ...s.activeAgentsByGroup, [groupId]: remainingActive },
      }
    }),

  clearAgentInFlight: (groupId, agentId, streamId = null) =>
    set((s) => {
      const groupInFlight = s.inFlightByGroup[groupId] ?? {}
      const remaining = { ...groupInFlight }
      delete remaining[inFlightKey(agentId, streamId)]
      if (streamId !== null) {
        delete remaining[inFlightKey(agentId, null)]
      }
      return {
        inFlightByGroup: {
          ...s.inFlightByGroup,
          [groupId]: remaining,
        },
      }
    }),

  setActiveAgent: (groupId, agent) =>
    set((s) => {
      const groupActive = s.activeAgentsByGroup[groupId] ?? {}
      const key = inFlightKey(agent.agent_id, agent.stream_id ?? null)
      return {
        activeAgentsByGroup: {
          ...s.activeAgentsByGroup,
          [groupId]: { ...groupActive, [key]: agent },
        },
      }
    }),

  clearActiveAgent: (groupId, agentId, streamId = null) =>
    set((s) => {
      if (!agentId) {
        if (streamId !== null) {
          const streamPrefix = `${streamId}:`
          const groupActive = s.activeAgentsByGroup[groupId] ?? {}
          const remaining = Object.fromEntries(
            Object.entries(groupActive).filter(([key]) => !key.startsWith(streamPrefix)),
          )
          return {
            activeAgentsByGroup: {
              ...s.activeAgentsByGroup,
              [groupId]: remaining,
            },
          }
        }
        return {
          activeAgentsByGroup: { ...s.activeAgentsByGroup, [groupId]: {} },
        }
      }
      const groupActive = s.activeAgentsByGroup[groupId] ?? {}
      const remaining = { ...groupActive }
      delete remaining[inFlightKey(agentId, streamId)]
      if (streamId !== null) {
        delete remaining[inFlightKey(agentId, null)]
      }
      return {
        activeAgentsByGroup: {
          ...s.activeAgentsByGroup,
          [groupId]: remaining,
        },
      }
    }),

  pushWarning: (groupId, warning) =>
    set((s) => ({
      warningsByGroup: {
        ...s.warningsByGroup,
        [groupId]: [...(s.warningsByGroup[groupId] ?? []), warning],
      },
    })),

  clearWarnings: (groupId) =>
    set((s) => ({
      warningsByGroup: { ...s.warningsByGroup, [groupId]: [] },
    })),

  pushToolActivity: (groupId, activity) =>
    set((s) => {
      const existing = s.toolActivityByGroup[groupId] ?? []
      const index = existing.findIndex((item) => item.id === activity.id)
      const next =
        index === -1
          ? [...existing, activity]
          : existing.map((item, itemIndex) =>
              itemIndex === index ? { ...item, ...activity } : item,
            )
      return {
        toolActivityByGroup: {
          ...s.toolActivityByGroup,
          [groupId]: next.slice(-8),
        },
      }
    }),

  clearToolActivity: (groupId) =>
    set((s) => ({
      toolActivityByGroup: { ...s.toolActivityByGroup, [groupId]: [] },
    })),

  startStreamRun: (groupId, streamId, userMessage) =>
    set((s) => {
      const groupRuns = s.streamRunsByGroup[groupId] ?? {}
      const groupOrder = s.streamRunOrderByGroup[groupId] ?? []
      const groupRunIdsByMessage = s.streamRunIdByUserMessageIdByGroup[groupId] ?? {}
      const timestamp = nowIso()
      const existing = groupRuns[streamId]
      const run: StreamRun = {
        id: streamId,
        group_id: groupId,
        user_message_id: userMessage.id,
        status: 'active',
        turn_id: existing?.turn_id ?? userMessage.turn_id,
        scheduler_status: existing?.scheduler_status ?? userMessage.turn_summary?.status ?? null,
        terminal_reason:
          existing?.terminal_reason ?? userMessage.turn_summary?.termination_reason ?? null,
        criticalSummaries: existing?.criticalSummaries ?? [],
        created_at: existing?.created_at ?? userMessage.created_at,
        updated_at: timestamp,
        events: existing?.events ?? [],
      }
      const nextRuns = { ...groupRuns, [streamId]: run }
      const nextOrder = groupOrder.includes(streamId)
        ? groupOrder
        : [...groupOrder, streamId]
      const pruned = pruneStreamRuns(nextRuns, nextOrder)
      const nextRunIdsByMessage = pruneStreamRunIdMap(
        groupRunIdsByMessage,
        pruned.removedIds,
      )
      return {
        streamRunsByGroup: { ...s.streamRunsByGroup, [groupId]: pruned.runs },
        streamRunIdByUserMessageIdByGroup: {
          ...s.streamRunIdByUserMessageIdByGroup,
          [groupId]: { ...nextRunIdsByMessage, [userMessage.id]: streamId },
        },
        streamRunOrderByGroup: { ...s.streamRunOrderByGroup, [groupId]: pruned.order },
      }
    }),

  addStreamAgentStart: (groupId, streamId, agent) =>
    set((s) => {
      const groupRuns = s.streamRunsByGroup[groupId] ?? {}
      const groupOrder = s.streamRunOrderByGroup[groupId] ?? []
      const timestamp = nowIso()
      const run = groupRuns[streamId] ?? emptyStreamRun(groupId, streamId, timestamp)
      const event: StreamAgentStartEvent = {
        id: `agent-start:${streamId}:${agent.agent_id}:${agent.round ?? 0}:${agent.index}`,
        type: 'agent_start',
        stream_id: streamId,
        agent_id: agent.agent_id,
        display_name: agent.display_name,
        index: agent.index,
        total: agent.total,
        round: agent.round,
        context_usage: agent.context_usage,
        created_at: timestamp,
      }
      const nextRun: StreamRun = {
        ...run,
        status: 'active',
        updated_at: timestamp,
        events: upsertTimelineEvent(run.events, event),
      }
      return {
        streamRunsByGroup: {
          ...s.streamRunsByGroup,
          [groupId]: { ...groupRuns, [streamId]: nextRun },
        },
        streamRunOrderByGroup: {
          ...s.streamRunOrderByGroup,
          [groupId]: groupOrder.includes(streamId) ? groupOrder : [...groupOrder, streamId],
        },
      }
    }),

  // Live per-turn usage: the backend emits `context_usage` right after the
  // prompt is built (before any tokens). Patch the latest agent_start event for
  // this agent so the avatar ring updates immediately; the final agent_message
  // later overrides it with the provider-exact figure.
  setStreamAgentContextUsage: (groupId, streamId, agentId, usage) =>
    set((s) => {
      const groupRuns = s.streamRunsByGroup[groupId]
      const run = groupRuns?.[streamId]
      if (!run) return {}
      let patchedIndex = -1
      for (let i = run.events.length - 1; i >= 0; i -= 1) {
        const ev = run.events[i]
        if (ev.type === 'agent_start' && ev.agent_id === agentId) {
          patchedIndex = i
          break
        }
      }
      if (patchedIndex === -1) return {}
      const events = run.events.map((ev, i) =>
        i === patchedIndex ? { ...ev, context_usage: usage } : ev,
      )
      return {
        streamRunsByGroup: {
          ...s.streamRunsByGroup,
          [groupId]: { ...groupRuns, [streamId]: { ...run, events } },
        },
      }
    }),

  patchStreamDraft: (groupId, streamId, agentId, delta, displayName) =>
    set((s) => {
      const groupRuns = s.streamRunsByGroup[groupId] ?? {}
      const groupOrder = s.streamRunOrderByGroup[groupId] ?? []
      const timestamp = nowIso()
      const run = groupRuns[streamId] ?? emptyStreamRun(groupId, streamId, timestamp)
      const nextRun: StreamRun = {
        ...run,
        status: 'active',
        updated_at: timestamp,
        events: appendTextSegment(run.events, streamId, agentId, delta, displayName, timestamp),
      }
      return {
        streamRunsByGroup: {
          ...s.streamRunsByGroup,
          [groupId]: { ...groupRuns, [streamId]: nextRun },
        },
        streamRunOrderByGroup: {
          ...s.streamRunOrderByGroup,
          [groupId]: groupOrder.includes(streamId) ? groupOrder : [...groupOrder, streamId],
        },
      }
    }),

  patchStreamReasoning: (groupId, streamId, agentId, delta, displayName) =>
    set((s) => {
      const groupRuns = s.streamRunsByGroup[groupId] ?? {}
      const groupOrder = s.streamRunOrderByGroup[groupId] ?? []
      const timestamp = nowIso()
      const run = groupRuns[streamId] ?? emptyStreamRun(groupId, streamId, timestamp)
      const last = run.events[run.events.length - 1]
      let events: StreamTimelineEvent[]
      if (
        last &&
        last.type === 'reasoning' &&
        last.agent_id === agentId &&
        last.status === 'streaming'
      ) {
        events = run.events.slice()
        events[run.events.length - 1] = {
          ...last,
          display_name: displayName ?? last.display_name,
          content: last.content + delta,
          updated_at: timestamp,
        }
      } else {
        const reasoning: StreamReasoningEvent = {
          id: `reasoning:${streamId}:${agentId}:${run.events.length}`,
          type: 'reasoning',
          stream_id: streamId,
          agent_id: agentId,
          display_name: displayName ?? 'Agent',
          content: delta,
          status: 'streaming',
          created_at: timestamp,
          updated_at: timestamp,
        }
        events = [...run.events, reasoning]
      }
      const nextRun: StreamRun = {
        ...run,
        status: 'active',
        updated_at: timestamp,
        events,
      }
      return {
        streamRunsByGroup: {
          ...s.streamRunsByGroup,
          [groupId]: { ...groupRuns, [streamId]: nextRun },
        },
        streamRunOrderByGroup: {
          ...s.streamRunOrderByGroup,
          [groupId]: groupOrder.includes(streamId) ? groupOrder : [...groupOrder, streamId],
        },
      }
    }),

  clearStreamingStreamDraft: (groupId, streamId, agentId) =>
    set((s) => {
      const groupRuns = s.streamRunsByGroup[groupId] ?? {}
      const run = groupRuns[streamId]
      if (!run) return {}
      const events = run.events.filter(
        (event) =>
          !(
            event.type === 'response_draft' &&
            event.agent_id === agentId &&
            event.status === 'streaming'
          ),
      )
      if (events.length === run.events.length) return {}
      const timestamp = nowIso()
      return {
        streamRunsByGroup: {
          ...s.streamRunsByGroup,
          [groupId]: {
            ...groupRuns,
            [streamId]: { ...run, updated_at: timestamp, events },
          },
        },
      }
    }),

  finalizeStreamDraft: (groupId, streamId, message, displayName) =>
    set((s) => {
      const agentId = message.sender_id ?? 'unknown-agent'
      const groupRuns = s.streamRunsByGroup[groupId] ?? {}
      const groupOrder = s.streamRunOrderByGroup[groupId] ?? []
      const timestamp = nowIso()
      const run = groupRuns[streamId] ?? emptyStreamRun(groupId, streamId, timestamp)
      let events = markReasoningDone(run.events, agentId)
      const segmentIds = events
        .filter(
          (event): event is StreamResponseDraftEvent =>
            event.type === 'response_draft' && event.agent_id === agentId,
        )
        .map((event) => event.id)
      if (segmentIds.length > 0) {
        const lastSegmentId = segmentIds[segmentIds.length - 1]
        events = events.map((event) => {
          if (event.type === 'response_draft' && event.agent_id === agentId) {
            const isFinalSegment = event.id === lastSegmentId
            return {
              ...event,
              display_name: displayName ?? event.display_name,
              content: isFinalSegment ? message.content ?? '' : event.content,
              status: 'finalized',
              message_id: isFinalSegment ? message.id : event.message_id,
              context_usage: message.context_usage ?? event.context_usage,
              updated_at: timestamp,
            }
          }
          return event
        })
      } else {
        const event: StreamAgentMessageEvent = {
          id: `agent-message:${streamId}:${message.id}`,
          type: 'agent_message',
          stream_id: streamId,
          message_id: message.id,
          agent_id: agentId,
          display_name: displayName ?? 'Agent',
          content: message.content ?? '',
          context_usage: message.context_usage,
          created_at: timestamp,
          updated_at: timestamp,
        }
        events = upsertTimelineEvent(events, event)
      }
      const nextRun: StreamRun = {
        ...run,
        updated_at: timestamp,
        events,
      }
      return {
        streamRunsByGroup: {
          ...s.streamRunsByGroup,
          [groupId]: { ...groupRuns, [streamId]: nextRun },
        },
        streamRunOrderByGroup: {
          ...s.streamRunOrderByGroup,
          [groupId]: groupOrder.includes(streamId) ? groupOrder : [...groupOrder, streamId],
        },
      }
    }),

  upsertStreamTool: (groupId, streamId, activity) =>
    set((s) => {
      const groupRuns = s.streamRunsByGroup[groupId] ?? {}
      const groupOrder = s.streamRunOrderByGroup[groupId] ?? []
      const timestamp = nowIso()
      const run = groupRuns[streamId] ?? emptyStreamRun(groupId, streamId, timestamp)
      const eventId = `tool:${streamId}:${activity.id}`
      const existing = run.events.find(
        (event): event is StreamToolEvent => event.id === eventId && event.type === 'tool',
      )
      const event: StreamToolEvent = {
        id: eventId,
        type: 'tool',
        stream_id: streamId,
        agent_id: activity.agent_id,
        display_name: activity.display_name,
        tool_call_id: activity.id,
        tool_name: activity.tool_name,
        status: activity.status,
        args_summary: activity.args_summary ?? existing?.args_summary,
        result_summary: activity.result_summary ?? existing?.result_summary,
        input_request: activity.input_request ?? existing?.input_request,
        created_at: existing?.created_at ?? timestamp,
        updated_at: timestamp,
      }
      const nextRun: StreamRun = {
        ...run,
        status: 'active',
        updated_at: timestamp,
        events: upsertTimelineEvent(markReasoningDone(run.events, activity.agent_id), event),
      }
      return {
        streamRunsByGroup: {
          ...s.streamRunsByGroup,
          [groupId]: { ...groupRuns, [streamId]: nextRun },
        },
        streamRunOrderByGroup: {
          ...s.streamRunOrderByGroup,
          [groupId]: groupOrder.includes(streamId) ? groupOrder : [...groupOrder, streamId],
        },
      }
    }),

  upsertStreamExternalRun: (groupId, streamId, eventInput) =>
    set((s) => {
      const groupRuns = s.streamRunsByGroup[groupId] ?? {}
      const groupOrder = s.streamRunOrderByGroup[groupId] ?? []
      const timestamp = nowIso()
      const run = groupRuns[streamId] ?? emptyStreamRun(groupId, streamId, timestamp)
      const eventId = `external:${streamId}:${eventInput.run_id}`
      const existing = run.events.find(
        (event): event is StreamExternalRunEvent =>
          event.id === eventId && event.type === 'external_run',
      )
      const event: StreamExternalRunEvent = {
        id: eventId,
        type: 'external_run',
        stream_id: streamId,
        run_id: eventInput.run_id,
        agent_id: eventInput.agent_id,
        display_name: eventInput.display_name,
        adapter: eventInput.adapter ?? existing?.adapter,
        status: eventInput.status ?? existing?.status,
        cwd: eventInput.cwd ?? existing?.cwd,
        exit_code: eventInput.exit_code ?? existing?.exit_code,
        summary: eventInput.summary ?? existing?.summary,
        created_at: existing?.created_at ?? timestamp,
        updated_at: timestamp,
      }
      const nextRun: StreamRun = {
        ...run,
        status: 'active',
        updated_at: timestamp,
        events: upsertTimelineEvent(markReasoningDone(run.events, eventInput.agent_id), event),
      }
      return {
        streamRunsByGroup: {
          ...s.streamRunsByGroup,
          [groupId]: { ...groupRuns, [streamId]: nextRun },
        },
        streamRunOrderByGroup: {
          ...s.streamRunOrderByGroup,
          [groupId]: groupOrder.includes(streamId) ? groupOrder : [...groupOrder, streamId],
        },
      }
    }),

  appendStreamNotice: (groupId, streamId, eventInput) =>
    set((s) => {
      const groupRuns = s.streamRunsByGroup[groupId] ?? {}
      const groupOrder = s.streamRunOrderByGroup[groupId] ?? []
      const timestamp = nowIso()
      const run = groupRuns[streamId] ?? emptyStreamRun(groupId, streamId, timestamp)
      const event: StreamNoticeEvent = {
        ...eventInput,
        id: `${eventInput.type}:${streamId}:${run.events.length}:${timestamp}`,
        stream_id: streamId,
        created_at: timestamp,
      }
      const baseEvents = eventInput.agent_id
        ? markReasoningDone(run.events, eventInput.agent_id)
        : run.events
      const nextRun: StreamRun = {
        ...run,
        updated_at: timestamp,
        events: [...baseEvents, event],
      }
      return {
        streamRunsByGroup: {
          ...s.streamRunsByGroup,
          [groupId]: { ...groupRuns, [streamId]: nextRun },
        },
        streamRunOrderByGroup: {
          ...s.streamRunOrderByGroup,
          [groupId]: groupOrder.includes(streamId) ? groupOrder : [...groupOrder, streamId],
        },
      }
    }),

  applySchedulerEvent: (groupId, streamId, update) => {
    if (update.stream_id !== streamId) return false
    const groupRuns = get().streamRunsByGroup[groupId] ?? {}
    const existing = groupRuns[streamId]
    const turnId = schedulerTurnId(update)
    if (existing?.turn_id && existing.turn_id !== turnId) {
      return false
    }
    const equivalentTerminalUpdate =
      update.kind === 'turn_completed' &&
      existing?.scheduler_status === update.payload.status &&
      existing.terminal_reason === update.payload.reason
    if (
      existing &&
      !schedulerStatusAcceptsEvents(existing.scheduler_status) &&
      update.kind !== 'done' &&
      !equivalentTerminalUpdate
    ) {
      return false
    }

    const timestamp = nowIso()
    const run = existing ?? emptyStreamRun(groupId, streamId, timestamp)
    const terminal = schedulerTerminalFields(update)
    const summary = schedulerSummary(update, timestamp)
    const nextRun: StreamRun = {
      ...run,
      turn_id: turnId,
      scheduler_status: terminal.status ?? run.scheduler_status,
      terminal_reason: terminal.status === null ? run.terminal_reason : terminal.reason,
      criticalSummaries: appendOrFoldSchedulerSummary(run.criticalSummaries, summary),
      updated_at: timestamp,
    }
    const groupOrder = get().streamRunOrderByGroup[groupId] ?? []
    set((s) => ({
      streamRunsByGroup: {
        ...s.streamRunsByGroup,
        [groupId]: { ...(s.streamRunsByGroup[groupId] ?? {}), [streamId]: nextRun },
      },
      streamRunOrderByGroup: {
        ...s.streamRunOrderByGroup,
        [groupId]: groupOrder.includes(streamId) ? groupOrder : [...groupOrder, streamId],
      },
    }))
    return true
  },

  linkStreamRunToUserMessage: (groupId, streamId, userMessageId) =>
    set((s) => {
      const groupRuns = s.streamRunsByGroup[groupId] ?? {}
      const run = groupRuns[streamId]
      if (!run) return {}
      const groupRunIdsByMessage =
        s.streamRunIdByUserMessageIdByGroup[groupId] ?? {}
      if (
        run.user_message_id === userMessageId &&
        groupRunIdsByMessage[userMessageId] === streamId
      ) {
        return {}
      }
      return {
        streamRunsByGroup: {
          ...s.streamRunsByGroup,
          [groupId]: {
            ...groupRuns,
            [streamId]: { ...run, user_message_id: userMessageId },
          },
        },
        streamRunIdByUserMessageIdByGroup: {
          ...s.streamRunIdByUserMessageIdByGroup,
          [groupId]: { ...groupRunIdsByMessage, [userMessageId]: streamId },
        },
      }
    }),

  detachStreamRun: (groupId, streamId) =>
    set((s) => {
      const streamPrefix = `${streamId}:`
      const groupInFlight = s.inFlightByGroup[groupId] ?? {}
      const nextInFlight = Object.fromEntries(
        Object.entries(groupInFlight).filter(([key]) => !key.startsWith(streamPrefix)),
      )
      const groupActive = s.activeAgentsByGroup[groupId] ?? {}
      const nextActive = Object.fromEntries(
        Object.entries(groupActive).filter(([key]) => !key.startsWith(streamPrefix)),
      )
      const groupRuns = s.streamRunsByGroup[groupId] ?? {}
      const run = groupRuns[streamId]
      if (!run || run.status !== 'active') {
        return {
          inFlightByGroup: { ...s.inFlightByGroup, [groupId]: nextInFlight },
          activeAgentsByGroup: { ...s.activeAgentsByGroup, [groupId]: nextActive },
        }
      }

      const nextRuns = { ...groupRuns }
      delete nextRuns[streamId]
      const groupRunIdsByMessage =
        s.streamRunIdByUserMessageIdByGroup[groupId] ?? {}
      const nextRunIdsByMessage = Object.fromEntries(
        Object.entries(groupRunIdsByMessage).filter(([, runId]) => runId !== streamId),
      )
      return {
        inFlightByGroup: { ...s.inFlightByGroup, [groupId]: nextInFlight },
        activeAgentsByGroup: { ...s.activeAgentsByGroup, [groupId]: nextActive },
        streamRunsByGroup: { ...s.streamRunsByGroup, [groupId]: nextRuns },
        streamRunIdByUserMessageIdByGroup: {
          ...s.streamRunIdByUserMessageIdByGroup,
          [groupId]: nextRunIdsByMessage,
        },
        streamRunOrderByGroup: {
          ...s.streamRunOrderByGroup,
          [groupId]: (s.streamRunOrderByGroup[groupId] ?? []).filter(
            (runId) => runId !== streamId,
          ),
        },
      }
    }),

  reconcileSchedulerTurn: (groupId, trace) =>
    set((s) => {
      const groupRuns = s.streamRunsByGroup[groupId] ?? {}
      const matchingRuns = Object.values(groupRuns).filter(
        (run) => run.turn_id === trace.turn.id,
      )
      if (matchingRuns.length === 0) return {}
      const timestamp = nowIso()
      const summary = reconciledTurnSummary(
        trace.turn.id,
        trace.turn.status,
        timestamp,
      )
      const nextRuns = { ...groupRuns }
      for (const run of matchingRuns) {
        nextRuns[run.id] = {
          ...run,
          status: streamStatusFromSchedulerStatus(trace.turn.status),
          scheduler_status: trace.turn.status,
          terminal_reason: trace.turn.termination_reason,
          criticalSummaries: appendOrFoldSchedulerSummary(
            run.criticalSummaries,
            summary,
          ),
          updated_at: timestamp,
        }
      }
      return {
        streamRunsByGroup: {
          ...s.streamRunsByGroup,
          [groupId]: nextRuns,
        },
      }
    }),

  acceptsStreamEvent: (groupId, streamId) => {
    const run = get().streamRunsByGroup[groupId]?.[streamId]
    return !run || schedulerStatusAcceptsEvents(run.scheduler_status)
  },

  markStreamRunWaitingForUser: (groupId, streamId) => {
    const groupRuns = get().streamRunsByGroup[groupId] ?? {}
    const run = groupRuns[streamId]
    if (!run?.turn_id || !schedulerStatusAcceptsEvents(run.scheduler_status)) {
      return null
    }
    if (
      run.scheduler_status === 'waiting_for_user' &&
      run.terminal_reason === 'waiting_for_user'
    ) {
      return null
    }
    const timestamp = nowIso()
    const summary: SchedulerCriticalSummary = {
      id: `waiting-for-user:${run.turn_id}`,
      kind: 'waiting_for_user',
      message: 'Turn is waiting for user input',
      count: 1,
      created_at: timestamp,
    }
    set((s) => ({
      streamRunsByGroup: {
        ...s.streamRunsByGroup,
        [groupId]: {
          ...(s.streamRunsByGroup[groupId] ?? {}),
          [streamId]: {
            ...run,
            scheduler_status: 'waiting_for_user',
            terminal_reason: 'waiting_for_user',
            criticalSummaries: appendOrFoldSchedulerSummary(
              run.criticalSummaries,
              summary,
            ),
            updated_at: timestamp,
          },
        },
      },
    }))
    return run.turn_id
  },

  markStreamRunDone: (groupId, streamId) =>
    set((s) => {
      const groupRuns = s.streamRunsByGroup[groupId] ?? {}
      const groupOrder = s.streamRunOrderByGroup[groupId] ?? []
      const groupRunIdsByMessage = s.streamRunIdByUserMessageIdByGroup[groupId] ?? {}
      const run = groupRuns[streamId]
      if (!run) return {}
      const timestamp = nowIso()
      const doneExists = run.events.some((event) => event.type === 'done')
      const doneEvent: StreamNoticeEvent = {
        id: `done:${streamId}`,
        type: 'done',
        stream_id: streamId,
        message: 'Stream completed',
        created_at: timestamp,
      }
      const nextRun: StreamRun = {
        ...run,
        status: 'completed',
        updated_at: timestamp,
        events: doneExists ? run.events : [...run.events, doneEvent],
      }
      const pruned = pruneStreamRuns({ ...groupRuns, [streamId]: nextRun }, groupOrder)
      return {
        streamRunsByGroup: { ...s.streamRunsByGroup, [groupId]: pruned.runs },
        streamRunIdByUserMessageIdByGroup: {
          ...s.streamRunIdByUserMessageIdByGroup,
          [groupId]: pruneStreamRunIdMap(groupRunIdsByMessage, pruned.removedIds),
        },
        streamRunOrderByGroup: { ...s.streamRunOrderByGroup, [groupId]: pruned.order },
      }
    }),

  markStreamRunError: (groupId, streamId, message) =>
    set((s) => {
      const groupRuns = s.streamRunsByGroup[groupId] ?? {}
      const groupOrder = s.streamRunOrderByGroup[groupId] ?? []
      const groupRunIdsByMessage = s.streamRunIdByUserMessageIdByGroup[groupId] ?? {}
      const run = groupRuns[streamId]
      if (!run) return {}
      const timestamp = nowIso()
      const errorEvent: StreamNoticeEvent = {
        id: `stream-error:${streamId}:${timestamp}`,
        type: 'agent_error',
        stream_id: streamId,
        message,
        created_at: timestamp,
      }
      const nextRun: StreamRun = {
        ...run,
        status: 'error',
        updated_at: timestamp,
        events: [...run.events, errorEvent],
      }
      const pruned = pruneStreamRuns({ ...groupRuns, [streamId]: nextRun }, groupOrder)
      return {
        streamRunsByGroup: { ...s.streamRunsByGroup, [groupId]: pruned.runs },
        streamRunIdByUserMessageIdByGroup: {
          ...s.streamRunIdByUserMessageIdByGroup,
          [groupId]: pruneStreamRunIdMap(groupRunIdsByMessage, pruned.removedIds),
        },
        streamRunOrderByGroup: { ...s.streamRunOrderByGroup, [groupId]: pruned.order },
      }
    }),

  markStreamRunCancelled: (groupId, streamIds) =>
    set((s) => {
      const groupRuns = s.streamRunsByGroup[groupId] ?? {}
      const groupOrder = s.streamRunOrderByGroup[groupId] ?? []
      const groupRunIdsByMessage = s.streamRunIdByUserMessageIdByGroup[groupId] ?? {}
      const ids = streamIds ??
        Object.values(groupRuns)
          .filter((run) => run.status === 'active')
          .map((run) => run.id)
      if (ids.length === 0) return {}
      const timestamp = nowIso()
      const nextRuns = { ...groupRuns }
      for (const streamId of ids) {
        const run = nextRuns[streamId]
        if (!run) continue
        if (run.turn_id && !schedulerStatusAcceptsEvents(run.scheduler_status)) continue
        const event: StreamNoticeEvent = {
          id: `cancelled:${streamId}:${timestamp}`,
          type: 'warning',
          stream_id: streamId,
          message: 'Stream cancelled',
          created_at: timestamp,
        }
        const schedulerCancelSummary: SchedulerCriticalSummary | null = run.turn_id
          ? {
              id: `scheduler-cancelled:${streamId}:${timestamp}`,
              kind: 'cancelled',
              message: 'Turn cancelled',
              count: 1,
              created_at: timestamp,
            }
          : null
        nextRuns[streamId] = {
          ...run,
          status: 'cancelled',
          scheduler_status: run.turn_id ? 'cancelled' : run.scheduler_status,
          terminal_reason: run.turn_id ? 'user_cancelled' : run.terminal_reason,
          criticalSummaries: appendOrFoldSchedulerSummary(
            run.criticalSummaries,
            schedulerCancelSummary,
          ),
          updated_at: timestamp,
          events: [...run.events, event],
        }
      }
      const pruned = pruneStreamRuns(nextRuns, groupOrder)
      return {
        streamRunsByGroup: { ...s.streamRunsByGroup, [groupId]: pruned.runs },
        streamRunIdByUserMessageIdByGroup: {
          ...s.streamRunIdByUserMessageIdByGroup,
          [groupId]: pruneStreamRunIdMap(groupRunIdsByMessage, pruned.removedIds),
        },
        streamRunOrderByGroup: { ...s.streamRunOrderByGroup, [groupId]: pruned.order },
      }
    }),

  appendToMessage: (groupId, messageId, delta) =>
    set((s) => {
      const list = s.byGroup[groupId] ?? []
      const next = list.map((m) =>
        m.id === messageId ? { ...m, content: (m.content ?? '') + delta } : m,
      )
      return { byGroup: { ...s.byGroup, [groupId]: next } }
    }),

  replaceMessage: (groupId, message) =>
    set((s) => {
      const list = s.byGroup[groupId] ?? []
      const next = list.map((m) => (m.id === message.id ? message : m))
      return { byGroup: { ...s.byGroup, [groupId]: next } }
    }),

  startResume: (messageId) =>
    set((s) => {
      const next = new Set(s.resumingMessageIds)
      next.add(messageId)
      return { resumingMessageIds: next }
    }),

  endResume: (messageId) =>
    set((s) => {
      const next = new Set(s.resumingMessageIds)
      next.delete(messageId)
      return { resumingMessageIds: next }
    }),
}))
