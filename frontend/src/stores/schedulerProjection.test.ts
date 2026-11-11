import { describe, expect, it } from 'vitest'
import type { SchedulerStreamUpdate } from '@/lib/api-v2/types'
import { acceptsSchedulerRunEvent, emptyStreamRun, projectSchedulerEvent } from './schedulerProjection'

const timestamp = '2026-11-11T00:00:00Z'
const budget = { agent_steps: 1, moderator_calls: 0, consecutive_failures: 0, total_failures: 0, total_tokens: 3 }
const completed: SchedulerStreamUpdate = {
  stream_id: 'stream', event_id: 'terminal', seq: 1, kind: 'turn_completed',
  payload: { turn_id: 'turn', status: 'failed', reason: 'persistence_failed', budget },
}

describe('pure Scheduler projection', () => {
  it('uses the supplied time and leaves the original run and event array intact', () => {
    const run = emptyStreamRun('group', 'stream', 'before')
    Object.freeze(run.events)
    Object.freeze(run.criticalSummaries)
    Object.freeze(run)
    const event: SchedulerStreamUpdate = {
      stream_id: 'stream', event_id: 'moderator', seq: 0, kind: 'moderator_started', payload: { turn_id: 'turn' },
    }
    expect(acceptsSchedulerRunEvent(run, event)).toBe(true)
    const next = projectSchedulerEvent('group', 'stream', run, event, timestamp)
    expect(next.updated_at).toBe(timestamp)
    expect(next.created_at).toBe('before')
    expect(next.moderator_active).toBe(true)
    expect(next.events).toBe(run.events)
    expect(run.turn_id).toBeNull()
    expect(run.updated_at).toBe('before')
  })

  it('rejects another turn and late dispatch but preserves equivalent terminal replay', () => {
    const terminal = projectSchedulerEvent('group', 'stream', undefined, completed, timestamp)
    const late: SchedulerStreamUpdate = {
      stream_id: 'stream', event_id: 'late', seq: 2, kind: 'moderator_started', payload: { turn_id: 'turn' },
    }
    expect(acceptsSchedulerRunEvent(terminal, late)).toBe(false)
    expect(acceptsSchedulerRunEvent({ ...terminal, turn_id: 'other' }, completed)).toBe(false)
    expect(acceptsSchedulerRunEvent(terminal, completed)).toBe(true)
    const replayed = projectSchedulerEvent('group', 'stream', terminal, completed, 'later')
    expect(replayed.criticalSummaries).toHaveLength(1)
    expect(replayed.scheduler_status).toBe('failed')
    expect(replayed.terminal_reason).toBe('persistence_failed')
    expect(terminal.updated_at).toBe(timestamp)
  })

  it('retains the existing bounded summary history for accepted events', () => {
    let run = emptyStreamRun('group', 'stream', timestamp)
    for (let seq = 0; seq < 21; seq++) {
      const event: SchedulerStreamUpdate = {
        stream_id: 'stream', event_id: `event-${seq}`, seq, kind: 'moderator_fallback',
        payload: { turn_id: 'turn', dispatch_id: `dispatch-${seq}`, target_agent_id: 'agent', reason: 'moderator_fallback' },
      }
      expect(acceptsSchedulerRunEvent(run, event)).toBe(true)
      run = projectSchedulerEvent('group', 'stream', run, event, timestamp)
    }
    expect(run.criticalSummaries).toHaveLength(20)
    expect(run.criticalSummaries[0].id).toBe('event-1')
    expect(run.criticalSummaries[19].id).toBe('event-20')
  })
})
