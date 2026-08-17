/**
 * "Your reply is ready" — the notification, and the rules for staying quiet.
 *
 * An agent turn can run for minutes, so the user leaves: another conversation,
 * another window, or the tray. A notification is only worth sending when they
 * are not already watching the conversation it is about, which is why this
 * checks both window focus and which conversation is on screen before firing.
 *
 * Enablement lives in localStorage rather than account settings: it pairs with
 * an OS-level permission that is granted per machine, so mirroring it to every
 * device the account signs in from would be the wrong promise.
 */

import i18n from '@/i18n'
import { showNotification } from '@/lib/notifications'
import {
  isConversationViewed,
  useConversationActivityStore,
  type ConversationActivityRun,
} from '@/stores/conversationActivityStore'

const ENABLED_STORAGE_KEY = 'ag-swarmer:notifications:reply-finished'

export type ReplyNotificationOutcome = 'completed' | 'waiting' | 'failed'

/** Enabled unless the user turned it off; a locked-down storage stays on. */
export function readReplyNotificationsEnabled(): boolean {
  try {
    return localStorage.getItem(ENABLED_STORAGE_KEY) !== 'false'
  } catch {
    return true
  }
}

export function writeReplyNotificationsEnabled(enabled: boolean): void {
  try {
    localStorage.setItem(ENABLED_STORAGE_KEY, String(enabled))
  } catch {
    // A preference that cannot be persisted still applies to this session.
  }
}

function conversationLabel(run: ConversationActivityRun): string {
  const fallback = i18n.t(
    run.scope === 'groups'
      ? 'chat:notifications.fallback.group'
      : 'chat:notifications.fallback.direct',
  )
  const conversation = run.conversation_title?.trim() || fallback
  const thread = run.thread_title?.trim()
  return thread ? `${conversation} · ${thread}` : conversation
}

function notificationBody(outcome: ReplyNotificationOutcome, detail?: string): string {
  if (outcome === 'failed' && detail?.trim()) {
    return i18n.t('chat:notifications.body.failedDetail', { message: detail.trim() })
  }
  return i18n.t(`chat:notifications.body.${outcome}`)
}

/** Whether the user is already looking at the conversation this is about. */
export function isReplyNotificationSuppressed(
  run: Pick<ConversationActivityRun, 'conversation_id' | 'thread_id'>,
): boolean {
  const { viewed } = useConversationActivityStore.getState()
  const focused = typeof document === 'undefined' ? false : document.hasFocus()
  return focused && isConversationViewed(viewed, run)
}

export function notifyReplyOutcome(
  run: ConversationActivityRun,
  outcome: ReplyNotificationOutcome,
  detail?: string,
): void {
  if (!readReplyNotificationsEnabled()) return
  if (isReplyNotificationSuppressed(run)) return
  void showNotification(conversationLabel(run), notificationBody(outcome, detail))
}
