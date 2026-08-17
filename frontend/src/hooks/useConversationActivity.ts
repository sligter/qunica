/**
 * Read the live activity status of a conversation or one of its task threads.
 *
 * Both selectors collapse to a single string, so a subscriber only re-renders
 * when the status it shows actually changes rather than on every token.
 */

import {
  selectConversationStatus,
  selectThreadStatus,
  useConversationActivityStore,
  type ConversationActivityStatus,
} from '@/stores/conversationActivityStore'

export function useConversationStatus(
  conversationId: string | undefined,
): ConversationActivityStatus | null {
  return useConversationActivityStore((state) =>
    selectConversationStatus(state, conversationId),
  )
}

export function useThreadStatus(
  conversationId: string | undefined,
  threadId: string | undefined,
): ConversationActivityStatus | null {
  return useConversationActivityStore((state) =>
    selectThreadStatus(state, conversationId, threadId),
  )
}
