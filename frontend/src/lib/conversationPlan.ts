import type { StreamRun } from '@/stores/messageStore'
import type { Message, TodoItem } from '@/types/api'

/**
 * The checklist the conversation is working through right now.
 *
 * `TodoWrite` replaces its list on every call, so only the newest list is
 * current: a live run's latest `todo` event, or the newest persisted reply that
 * kept one. A plan whose items are all done has nothing left to track and is
 * not returned, so the panel above the composer clears itself when the work
 * finishes instead of lingering over the next question.
 */
export function latestConversationPlan(
  messages: readonly Message[] | undefined,
  runs: Record<string, StreamRun> | undefined,
): TodoItem[] {
  const candidates: { at: string; todos: TodoItem[] }[] = []
  for (const message of messages ?? []) {
    if (message.sender_type === 'agent' && message.todos?.length) {
      candidates.push({ at: message.created_at, todos: message.todos })
    }
  }
  for (const run of Object.values(runs ?? {})) {
    for (const event of run.events) {
      if (event.type === 'todo' && event.todos.length > 0) {
        candidates.push({ at: event.created_at, todos: event.todos })
      }
    }
  }
  let latest: TodoItem[] = []
  let latestAt = ''
  for (const candidate of candidates) {
    if (candidate.at >= latestAt) {
      latest = candidate.todos
      latestAt = candidate.at
    }
  }
  return latest.every((todo) => todo.status === 'completed') ? [] : latest
}
