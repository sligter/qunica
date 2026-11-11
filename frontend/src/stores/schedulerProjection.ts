// Pure Scheduler projection. The Store owns stream retirement and transport identity.
// Type-only Store imports preserve its existing public type paths; no runtime cycle.
import type { GroupTurnStatus, GroupTurnTerminationReason, SchedulerStreamUpdate } from '@/lib/api-v2/types'
import type { SchedulerCriticalSummary, StreamRun, StreamRunStatus } from './messageStore'

const MAX_CRITICAL_SUMMARIES_PER_RUN = 20

export function emptyStreamRun(groupId: string, streamId: string, timestamp: string): StreamRun {
  return {
    id: streamId,
    group_id: groupId,
    user_message_id: streamId,
    status: 'active',
    turn_id: null,
    scheduler_status: null,
    moderator_active: false,
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
    case 'moderator_started':
    case 'done':
      return null
  }
}

export function appendOrFoldSchedulerSummary(
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
    case 'moderator_started':
    case 'moderator_fallback':
    case 'done':
      return { status: null, reason: null }
  }
}

function schedulerTurnId(update: SchedulerStreamUpdate): string {
  return update.payload.turn_id
}

export function schedulerStatusAcceptsEvents(status: GroupTurnStatus | null): boolean {
  return (
    status === null ||
    status === 'pending' ||
    status === 'running' ||
    status === 'waiting_for_user'
  )
}

export function streamStatusFromSchedulerStatus(status: GroupTurnStatus): StreamRunStatus {
  if (status === 'cancelled') return 'cancelled'
  if (status === 'failed') return 'error'
  if (status === 'queued' || status === 'pending' || status === 'running' || status === 'waiting_for_user') {
    return 'active'
  }
  return 'completed'
}

export function acceptsSchedulerRunEvent(existing: StreamRun | undefined, update: SchedulerStreamUpdate): boolean {
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

    return true
}

export function projectSchedulerEvent(groupId: string, streamId: string, existing: StreamRun | undefined, update: SchedulerStreamUpdate, timestamp: string): StreamRun {
    const turnId = schedulerTurnId(update)
    const run = existing ?? emptyStreamRun(groupId, streamId, timestamp)
    const terminal = schedulerTerminalFields(update)
    const summary = schedulerSummary(update, timestamp)
    const nextRun: StreamRun = {
      ...run,
      turn_id: turnId,
      // Note: terminal scheduler events precede `done`; stop activity now even
      // when an adapter never sends the last tool result or the stream closes.
      status: terminal.status === null ? run.status : streamStatusFromSchedulerStatus(terminal.status),
      scheduler_status: terminal.status ?? run.scheduler_status,
      moderator_active: update.kind === 'moderator_started',
      terminal_reason: terminal.status === null ? run.terminal_reason : terminal.reason,
      criticalSummaries: appendOrFoldSchedulerSummary(run.criticalSummaries, summary),
      updated_at: timestamp,
    }
    return nextRun
}
