import { useMemo } from 'react'

import { TodoChecklist } from '@/components/chat/TodoChecklist'
import { latestConversationPlan } from '@/lib/conversationPlan'
import { useMessageStore } from '@/stores/messageStore'

/**
 * The agent's current plan, pinned above the composer.
 *
 * Note: the plan lives here rather than in the reply because it is the state of
 * the work, not part of the answer — inside a bubble it scrolls away with the
 * reply that first wrote it while later `TodoWrite` calls keep updating it.
 */
export function ConversationPlan({ stateId }: { stateId: string }) {
  const messages = useMessageStore((state) => state.byGroup[stateId])
  const runs = useMessageStore((state) => state.streamRunsByGroup[stateId])
  const todos = useMemo(() => latestConversationPlan(messages, runs), [messages, runs])
  if (todos.length === 0) return null
  return (
    <div className="shrink-0 px-4 pb-2">
      <TodoChecklist
        collapsible
        todos={todos}
        className="mx-auto max-h-48 w-full max-w-6xl overflow-y-auto bg-card"
      />
    </div>
  )
}
