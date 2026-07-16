import { describe, expect, it, vi } from 'vitest'

import { parseGroupTurnTrace, parseSchedulerStreamEvent } from './schemas'
import type { StreamEvent, StreamEventKind } from './types'
import type { Message } from '@/types/api'

const budgetUsage = {
  agent_steps: 2,
  moderator_calls: 1,
  consecutive_failures: 0,
  total_failures: 0,
  total_tokens: 128,
}

const budgetLimits = {
  max_agent_steps: 8,
  max_steps_per_agent: 3,
  max_hops: 5,
  max_moderator_calls: 4,
  max_consecutive_failures: 3,
  max_total_failures: 6,
  max_total_tokens: 120000,
}

function schedulerEvent(
  kind: StreamEventKind,
  payload: unknown,
): StreamEvent<unknown, StreamEventKind> {
  return {
    stream_id: 'stream-1',
    seq: 1,
    event_id: 'stream-1:1',
    kind,
    payload,
  }
}

function traceFixture(childArtifact: unknown = {
  mode: 'handoff',
  target_agent_id: 'agent-2',
  child_dispatch_id: 'dispatch-child',
}): unknown {
  return {
    turn: {
      id: 'turn-1',
      thread_id: 'thread-1',
      group_id: 'group-1',
      trigger_message_id: 'message-1',
      status: 'completed',
      scheduler_strategy: 'bounded',
      config_snapshot: { max_agent_steps: 8 },
      topology_snapshot: { kind: 'mesh' },
      ...budgetUsage,
      termination_reason: null,
      created_at: '2026-07-15T00:00:00Z',
      started_at: '2026-07-15T00:00:01Z',
      completed_at: '2026-07-15T00:00:02Z',
      updated_at: '2026-07-15T00:00:02Z',
    },
    budget: budgetUsage,
    dispatches: [
      {
        id: 'dispatch-root',
        turn_id: 'turn-1',
        parent_dispatch_id: null,
        source_agent_id: null,
        target_agent_id: 'agent-1',
        selection_reason: 'user_mention',
        action_kind: 'speak',
        hop: 0,
        status: 'completed',
        input_message_id: 'message-1',
        output_message_id: 'message-2',
        artifact: null,
        total_tokens: 64,
        failure_code: null,
        created_at: '2026-07-15T00:00:01Z',
        started_at: '2026-07-15T00:00:01Z',
        completed_at: '2026-07-15T00:00:02Z',
        updated_at: '2026-07-15T00:00:02Z',
      },
      {
        id: 'dispatch-child',
        turn_id: 'turn-1',
        parent_dispatch_id: 'dispatch-root',
        source_agent_id: 'agent-1',
        target_agent_id: 'agent-2',
        selection_reason: 'agent_handoff',
        action_kind: 'handoff',
        hop: 1,
        status: 'completed',
        input_message_id: 'message-2',
        output_message_id: 'message-3',
        artifact: childArtifact,
        total_tokens: 64,
        failure_code: null,
        created_at: '2026-07-15T00:00:02Z',
        started_at: '2026-07-15T00:00:02Z',
        completed_at: '2026-07-15T00:00:03Z',
        updated_at: '2026-07-15T00:00:03Z',
      },
    ],
    estimated_cost: null,
    cost_estimation_status: 'unavailable',
  }
}

describe('parseSchedulerStreamEvent', () => {
  it.each([
    ['turn_started', { turn_id: 'turn-1', budget: budgetLimits }],
    [
      'speaker_selected',
      {
        turn_id: 'turn-1',
        dispatch_id: 'dispatch-1',
        source_agent_id: null,
        target_agent_id: 'agent-1',
        reason: 'user_mention',
        action_kind: 'speak',
        hop: 0,
      },
    ],
    [
      'dispatch_failed',
      {
        turn_id: 'turn-1',
        dispatch_id: 'dispatch-1',
        target_agent_id: 'agent-1',
        action_kind: 'call',
        reason: 'persistence_failed',
      },
    ],
    [
      'moderator_fallback',
      {
        turn_id: 'turn-1',
        dispatch_id: 'dispatch-1',
        target_agent_id: 'agent-1',
        reason: 'moderator_fallback',
      },
    ],
    [
      'turn_cancelled',
      { turn_id: 'turn-1', status: 'cancelled', reason: 'user_cancelled', budget: budgetUsage },
    ],
    [
      'turn_superseded',
      { turn_id: 'turn-1', status: 'superseded', reason: 'superseded', budget: budgetUsage },
    ],
    [
      'turn_budget_exhausted',
      {
        turn_id: 'turn-1',
        status: 'budget_exhausted',
        reason: 'budget_exhausted',
        budget: budgetUsage,
      },
    ],
    [
      'turn_completed',
      {
        turn_id: 'turn-1',
        status: 'completed',
        reason: null,
        budget: { ...budgetUsage, limits: budgetLimits },
      },
    ],
    ['done', { turn_id: 'turn-1' }],
  ] satisfies ReadonlyArray<readonly [StreamEventKind, unknown]>)(
    'parses the %s scheduler payload',
    (kind, payload) => {
      expect(parseSchedulerStreamEvent(schedulerEvent(kind, payload))?.kind).toBe(kind)
    },
  )

  it('returns null silently for legacy events and empty legacy done payloads', () => {
    const warning = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    expect(parseSchedulerStreamEvent(schedulerEvent('token', { text: 'legacy' }))).toBeNull()
    expect(parseSchedulerStreamEvent(schedulerEvent('done', {}))).toBeNull()
    expect(warning).not.toHaveBeenCalled()

    warning.mockRestore()
  })

  it('rejects malformed scheduler status, reason, and action codes safely', () => {
    const warning = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    expect(
      parseSchedulerStreamEvent(
        schedulerEvent('speaker_selected', {
          turn_id: 'turn-1',
          dispatch_id: 'dispatch-1',
          source_agent_id: null,
          target_agent_id: 'agent-1',
          reason: 'user_mention',
          action_kind: 'message',
          hop: 0,
        }),
      ),
    ).toBeNull()
    expect(
      parseSchedulerStreamEvent(
        schedulerEvent('turn_completed', {
          turn_id: 'turn-1',
          status: 'done',
          reason: 'unknown',
          budget: budgetUsage,
        }),
      ),
    ).toBeNull()
    expect(warning).toHaveBeenCalledTimes(2)

    warning.mockRestore()
  })

  it.each([
    [
      'turn_cancelled',
      { turn_id: 'turn-1', status: 'completed', reason: 'user_cancelled', budget: budgetUsage },
    ],
    [
      'turn_superseded',
      { turn_id: 'turn-1', status: 'superseded', reason: 'user_cancelled', budget: budgetUsage },
    ],
    [
      'turn_budget_exhausted',
      {
        turn_id: 'turn-1',
        status: 'budget_exhausted',
        reason: 'failure_budget_exhausted',
        budget: budgetUsage,
      },
    ],
    [
      'turn_completed',
      { turn_id: 'turn-1', status: 'completed', reason: 'silence', budget: budgetUsage },
    ],
    [
      'turn_completed',
      { turn_id: 'turn-1', status: 'cancelled', reason: 'user_cancelled', budget: budgetUsage },
    ],
  ] satisfies ReadonlyArray<readonly [StreamEventKind, unknown]>)(
    'rejects invalid %s terminal status and reason combinations',
    (kind, payload) => {
      const warning = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

      expect(parseSchedulerStreamEvent(schedulerEvent(kind, payload))).toBeNull()
      expect(warning).toHaveBeenCalledOnce()

      warning.mockRestore()
    },
  )
})

describe('parseGroupTurnTrace', () => {
  it('parses ordered dispatch DAG data and unavailable cost metadata', () => {
    const trace = parseGroupTurnTrace(traceFixture())

    expect(trace.dispatches.map((dispatch) => dispatch.id)).toEqual([
      'dispatch-root',
      'dispatch-child',
    ])
    expect(trace.dispatches[1]?.parent_dispatch_id).toBe('dispatch-root')
    expect(trace.estimated_cost).toBeNull()
    expect(trace.cost_estimation_status).toBe('unavailable')
  })

  it.each([
    ['empty artifacts', {}],
    ['final content', { final_content: 'private' }],
    ['tool input and output', { tool_io: { input: 'private', output: 'private' } }],
    ['reasoning', { reasoning: 'private' }],
    ['usage', { usage: { total_tokens: 42 } }],
    ['unknown fields', { unexpected: 'private' }],
  ] satisfies ReadonlyArray<readonly [string, unknown]>)('rejects %s from public artifacts', (
    _label,
    artifact,
  ) => {
    expect(() =>
      parseGroupTurnTrace(traceFixture(artifact)),
    ).toThrow()
  })
})

describe('legacy message causality', () => {
  it('keeps scheduler causality fields nullable for pre-scheduler messages', () => {
    const message: Message = {
      id: 'message-legacy',
      group_id: 'group-1',
      thread_id: null,
      sender_type: 'user',
      sender_id: 'user-1',
      message_type: 'text',
      content: 'legacy message',
      status: 'visible',
      refs: null,
      context_usage: null,
      reply_to_message_id: null,
      turn_id: null,
      dispatch_id: null,
      turn_summary: null,
      created_at: '2026-07-15T00:00:00Z',
    }

    expect(message.turn_id).toBeNull()
    expect(message.dispatch_id).toBeNull()
    expect(message.reply_to_message_id).toBeNull()
  })
})
