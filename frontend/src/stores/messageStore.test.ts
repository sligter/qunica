import { beforeEach, describe, expect, it } from 'vitest'

import type { SchedulerStreamUpdate } from '@/lib/api-v2/types'
import { useMessageStore } from '@/stores/messageStore'
import type { Message } from '@/types/api'

const initialState = useMessageStore.getInitialState()

function message(id: string): Message {
  return {
    id,
    group_id: 'group-1',
    thread_id: 'thread-1',
    sender_type: 'user',
    sender_id: 'user-1',
    message_type: 'text',
    content: id,
    status: 'visible',
    refs: null,
    context_usage: null,
    turn_id: null,
    dispatch_id: null,
    reply_to_message_id: null,
    turn_summary: null,
    created_at: '2026-07-15T00:00:00Z',
  }
}

function update(
  kind: SchedulerStreamUpdate['kind'],
  payload: SchedulerStreamUpdate['payload'],
  seq: number,
): SchedulerStreamUpdate {
  return {
    stream_id: 'stream-1',
    seq,
    event_id: `event-${seq}`,
    kind,
    payload,
  } as SchedulerStreamUpdate
}

describe('messageStore scheduler state', () => {
  beforeEach(() => {
    useMessageStore.setState(initialState, true)
  })

  it('isolates turns and bubbles by stream plus agent and rejects superseded writes', () => {
    const store = useMessageStore.getState()
    store.startStreamRun('group-1', 'stream-1', message('message-1'))
    store.startStreamRun('group-1', 'stream-2', message('message-2'))
    store.applySchedulerEvent(
      'group-1',
      'stream-1',
      update('turn_started', {
        turn_id: 'turn-1',
        budget: {
          max_agent_steps: 8,
          max_steps_per_agent: 3,
          max_hops: 4,
          max_moderator_calls: 2,
          max_consecutive_failures: 2,
          max_total_failures: 4,
          max_total_tokens: 1000,
        },
      }, 1),
    )
    store.applySchedulerEvent(
      'group-1',
      'stream-2',
      { ...update('turn_started', {
        turn_id: 'turn-2',
        budget: {
          max_agent_steps: 8,
          max_steps_per_agent: 3,
          max_hops: 4,
          max_moderator_calls: 2,
          max_consecutive_failures: 2,
          max_total_failures: 4,
          max_total_tokens: 1000,
        },
      }, 2), stream_id: 'stream-2' },
    )
    store.patchInFlight('group-1', 'agent-1', 'old', 'stream-1')
    store.patchInFlight('group-1', 'agent-1', 'new', 'stream-2')
    store.applySchedulerEvent(
      'group-1',
      'stream-1',
      update('turn_superseded', {
        turn_id: 'turn-1',
        status: 'superseded',
        reason: 'superseded',
        budget: {
          agent_steps: 1,
          moderator_calls: 0,
          consecutive_failures: 0,
          total_failures: 0,
          total_tokens: 12,
        },
      }, 3),
    )

    const state = useMessageStore.getState()
    expect(state.streamRunsByGroup['group-1']['stream-1'].turn_id).toBe('turn-1')
    expect(state.streamRunsByGroup['group-1']['stream-2'].turn_id).toBe('turn-2')
    expect(state.inFlightByGroup['group-1']['stream-1:agent-1'].content).toBe('old')
    expect(state.inFlightByGroup['group-1']['stream-2:agent-1'].content).toBe('new')
    expect(state.acceptsStreamEvent('group-1', 'stream-1')).toBe(false)
    expect(state.acceptsStreamEvent('group-1', 'stream-2')).toBe(true)
  })

  it('folds routine selections while retaining bounded critical summaries', () => {
    const store = useMessageStore.getState()
    store.startStreamRun('group-1', 'stream-1', message('message-1'))
    store.applySchedulerEvent(
      'group-1',
      'stream-1',
      update('turn_started', {
        turn_id: 'turn-1',
        budget: {
          max_agent_steps: 8,
          max_steps_per_agent: 3,
          max_hops: 4,
          max_moderator_calls: 2,
          max_consecutive_failures: 2,
          max_total_failures: 4,
          max_total_tokens: 1000,
        },
      }, 1),
    )
    for (let seq = 2; seq <= 3; seq += 1) {
      store.applySchedulerEvent(
        'group-1',
        'stream-1',
        update('speaker_selected', {
          turn_id: 'turn-1',
          dispatch_id: `dispatch-${seq}`,
          source_agent_id: null,
          target_agent_id: `agent-${seq}`,
          reason: 'deterministic_order',
          action_kind: 'speak',
          hop: 0,
        }, seq),
      )
    }
    store.applySchedulerEvent(
      'group-1',
      'stream-1',
      update('speaker_selected', {
        turn_id: 'turn-1',
        dispatch_id: 'dispatch-call',
        source_agent_id: 'agent-1',
        target_agent_id: 'agent-helper',
        reason: 'agent_call',
        action_kind: 'call',
        hop: 1,
      }, 4),
    )
    store.applySchedulerEvent(
      'group-1',
      'stream-1',
      update('speaker_selected', {
        turn_id: 'turn-1',
        dispatch_id: 'dispatch-handoff',
        source_agent_id: 'agent-1',
        target_agent_id: 'agent-next',
        reason: 'agent_handoff',
        action_kind: 'handoff',
        hop: 1,
      }, 5),
    )
    store.applySchedulerEvent(
      'group-1',
      'stream-1',
      update('moderator_fallback', {
        turn_id: 'turn-1',
        dispatch_id: 'dispatch-fallback',
        target_agent_id: 'agent-fallback',
        reason: 'moderator_fallback',
      }, 6),
    )
    store.applySchedulerEvent(
      'group-1',
      'stream-1',
      update('turn_budget_exhausted', {
        turn_id: 'turn-1',
        status: 'budget_exhausted',
        reason: 'budget_exhausted',
        budget: {
          agent_steps: 8,
          moderator_calls: 1,
          consecutive_failures: 0,
          total_failures: 0,
          total_tokens: 999,
        },
      }, 7),
    )

    const run = useMessageStore.getState().streamRunsByGroup['group-1']['stream-1']
    expect(run.criticalSummaries.map((summary) => summary.kind)).toEqual([
      'deterministic_selection',
      'call',
      'handoff',
      'moderator_fallback',
      'budget_exhausted',
    ])
    expect(run.criticalSummaries[0]).toMatchObject({ count: 2, message: 'Scheduler selected 2 speakers' })
    expect(run.criticalSummaries.every((summary) => !('dispatch_id' in summary))).toBe(true)
    expect(run.scheduler_status).toBe('budget_exhausted')
    expect(run.terminal_reason).toBe('budget_exhausted')

    store.clearGroupMessages('group-1')
    store.startStreamRun('group-1', 'stream-1', message('message-2'))
    store.applySchedulerEvent(
      'group-1',
      'stream-1',
      update('turn_started', {
        turn_id: 'turn-2',
        budget: {
          max_agent_steps: 30,
          max_steps_per_agent: 30,
          max_hops: 4,
          max_moderator_calls: 2,
          max_consecutive_failures: 30,
          max_total_failures: 30,
          max_total_tokens: 1000,
        },
      }, 8),
    )
    for (let seq = 8; seq < 36; seq += 1) {
      store.applySchedulerEvent(
        'group-1',
        'stream-1',
        update('dispatch_failed', {
          turn_id: 'turn-2',
          dispatch_id: `dispatch-${seq}`,
          target_agent_id: `agent-${seq}`,
          action_kind: 'speak',
          reason: 'persistence_failed',
        }, seq),
      )
    }
    expect(
      useMessageStore.getState().streamRunsByGroup['group-1']['stream-1'].criticalSummaries,
    ).toHaveLength(20)
  })

  it('preserves an existing scheduler terminal status during local cancellation', () => {
    const store = useMessageStore.getState()
    store.startStreamRun('group-1', 'stream-1', message('message-1'))
    store.applySchedulerEvent(
      'group-1',
      'stream-1',
      update('turn_started', {
        turn_id: 'turn-1',
        budget: {
          max_agent_steps: 8,
          max_steps_per_agent: 3,
          max_hops: 4,
          max_moderator_calls: 2,
          max_consecutive_failures: 2,
          max_total_failures: 4,
          max_total_tokens: 1000,
        },
      }, 1),
    )
    store.applySchedulerEvent(
      'group-1',
      'stream-1',
      update('turn_superseded', {
        turn_id: 'turn-1',
        status: 'superseded',
        reason: 'superseded',
        budget: {
          agent_steps: 1,
          moderator_calls: 0,
          consecutive_failures: 0,
          total_failures: 0,
          total_tokens: 10,
        },
      }, 2),
    )

    store.markStreamRunCancelled('group-1', ['stream-1'])

    const run = useMessageStore.getState().streamRunsByGroup['group-1']['stream-1']
    expect(run.scheduler_status).toBe('superseded')
    expect(run.terminal_reason).toBe('superseded')
    expect(run.criticalSummaries.map((summary) => summary.kind)).toEqual(['superseded'])
  })

  it('records waiting once when a later terminal update repeats the same state', () => {
    const store = useMessageStore.getState()
    store.startStreamRun('group-1', 'stream-1', message('message-1'))
    store.applySchedulerEvent(
      'group-1',
      'stream-1',
      update('turn_started', {
        turn_id: 'turn-1',
        budget: {
          max_agent_steps: 8,
          max_steps_per_agent: 3,
          max_hops: 4,
          max_moderator_calls: 2,
          max_consecutive_failures: 2,
          max_total_failures: 4,
          max_total_tokens: 1000,
        },
      }, 1),
    )

    expect(store.markStreamRunWaitingForUser('group-1', 'stream-1')).toBe('turn-1')
    expect(store.markStreamRunWaitingForUser('group-1', 'stream-1')).toBeNull()
    expect(
      store.applySchedulerEvent(
        'group-1',
        'stream-1',
        update('turn_completed', {
          turn_id: 'turn-1',
          status: 'waiting_for_user',
          reason: 'waiting_for_user',
          budget: {
            agent_steps: 1,
            moderator_calls: 0,
            consecutive_failures: 0,
            total_failures: 0,
            total_tokens: 10,
          },
        }, 2),
      ),
    ).toBe(true)

    const run = useMessageStore.getState().streamRunsByGroup['group-1']['stream-1']
    expect(run.scheduler_status).toBe('waiting_for_user')
    expect(run.terminal_reason).toBe('waiting_for_user')
    expect(run.criticalSummaries.map((summary) => summary.kind)).toEqual([
      'waiting_for_user',
    ])
  })
})
