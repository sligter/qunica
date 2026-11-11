import { describe, expect, it, vi } from 'vitest'
import wireFixture from '../../../../backend-rs/crates/domain/src/fixtures/speaker_selected.json?raw'
import { parseSchedulerStreamEvent } from './schemas'
import type { StreamEvent, StreamEventKind } from './types'

const fixture: StreamEvent<unknown, 'speaker_selected'> = JSON.parse(wireFixture)

describe('shared Rust speaker_selected wire fixture', () => {
  it('preserves the complete event and its required nullable source', () => {
    expect(parseSchedulerStreamEvent(fixture)).toEqual(fixture)
    const parsed = parseSchedulerStreamEvent(fixture)
    expect(parsed?.kind).toBe('speaker_selected')
    if (parsed?.kind !== 'speaker_selected') throw new Error('fixture must be a selection')
    expect(parsed.payload.source_agent_id).toBeNull()
    const withSource = { ...fixture, payload: { ...parsed.payload, source_agent_id: 'source-1' } }
    expect(parseSchedulerStreamEvent(withSource)).toEqual(withSource)
  })

  it.each(['missing source', 'undefined source', 'unknown field', 'unknown reason', 'fractional hop'])(
    'rejects %s without publishing a partial event', (caseName) => {
      const parsed = parseSchedulerStreamEvent(fixture)
      if (parsed?.kind !== 'speaker_selected') throw new Error('fixture must be a selection')
      const payload: Record<string, unknown> = { ...parsed.payload }
      if (caseName === 'missing source') delete payload.source_agent_id
      if (caseName === 'undefined source') payload.source_agent_id = undefined
      if (caseName === 'unknown field') payload.extra = true
      if (caseName === 'unknown reason') payload.reason = 'future_reason'
      if (caseName === 'fractional hop') payload.hop = 0.5
      const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
      try { expect(parseSchedulerStreamEvent({ ...fixture, payload })).toBeNull() }
      finally { warn.mockRestore() }
    },
  )

  it('ignores a future event kind and retains the legacy empty done payload', () => {
    // Deliberately malformed input models a newer server at the wire boundary.
    const future: StreamEvent<unknown, StreamEventKind> = JSON.parse(
      JSON.stringify({ ...fixture, kind: 'future_scheduler_event' }),
    )
    expect(parseSchedulerStreamEvent(future)).toBeNull()
    expect(parseSchedulerStreamEvent({ ...fixture, kind: 'done', payload: {} })).toBeNull()
  })
})
