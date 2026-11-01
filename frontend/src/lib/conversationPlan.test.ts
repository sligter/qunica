import { describe, expect, it } from 'vitest'

import { latestConversationPlan } from '@/lib/conversationPlan'
import type { StreamRun } from '@/stores/messageStore'
import type { Message, TodoItem } from '@/types/api'

function message(created_at: string, todos: TodoItem[]): Message {
  return { id: created_at, sender_type: 'agent', created_at, todos } as Message
}

function run(created_at: string, todos: TodoItem[]): Record<string, StreamRun> {
  return {
    'stream-1': {
      events: [{ id: 'todo-1', type: 'todo', agent_id: 'a', display_name: 'A', todos, created_at }],
    } as unknown as StreamRun,
  }
}

const open: TodoItem[] = [{ content: 'write the fix', status: 'in_progress' }]

describe('latestConversationPlan', () => {
  it('prefers a live update over an older persisted plan', () => {
    const older = [{ content: 'old step', status: 'pending' as const }]
    expect(latestConversationPlan([message('2026-01-01T00:00:00Z', older)], run('2026-01-01T00:01:00Z', open)))
      .toEqual(open)
  })

  it('clears once every item is done', () => {
    const done: TodoItem[] = [{ content: 'write the fix', status: 'completed' }]
    expect(latestConversationPlan([message('2026-01-01T00:00:00Z', open), message('2026-01-01T00:02:00Z', done)], undefined))
      .toEqual([])
  })

  it('is empty when no agent kept a plan', () => {
    expect(latestConversationPlan([], undefined)).toEqual([])
  })
})
