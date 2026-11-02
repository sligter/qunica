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

import { useAccountSyncStore } from '@/stores/accountSyncStore'
import i18n from '@/i18n'
import { showNotification } from '@/lib/notifications'
import {
  isConversationViewed,
  useConversationActivityStore,
  type ConversationActivityRun,
} from '@/stores/conversationActivityStore'

const ENABLED_STORAGE_KEY = 'qunica:notifications:reply-finished'

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

/** A watchdog check's alert about a task, sent unless the user is watching its conversation. */
export function notifyWatchdogAlert(
  target: { conversation_id: string; thread_id: string | null },
  alert: { title?: string | null; action?: string; message?: string },
): void {
  if (!readReplyNotificationsEnabled()) return
  if (isReplyNotificationSuppressed(target)) return
  const title = alert.title?.trim() || i18n.t('chat:taskReport.untitled')
  const heading = i18n.t(
    alert.action === 'stopped'
      ? 'chat:notifications.watchdog.stopped'
      : 'chat:notifications.watchdog.notified',
    { title },
  )
  void showNotification(heading, alert.message ?? '').then((result) => {
    if (!result.ok) console.warn('Watchdog notification was not delivered:', result.error)
  })
}

export type ReplyNotificationKind = 'main' | 'task' | 'watchdog'
export type TaskNotificationLevel = 'summary' | 'all'
const TASK_STORAGE_KEY = 'qunica:notifications:task-updates'

export function readTaskNotificationLevel(): TaskNotificationLevel {
  try { return localStorage.getItem(TASK_STORAGE_KEY) === 'all' ? 'all' : 'summary' }
  catch { return 'summary' }
}
export function writeTaskNotificationLevel(level: TaskNotificationLevel): void {
  try { localStorage.setItem(TASK_STORAGE_KEY, level) }
  catch { /* Unavailable storage retains the summary default. */ }
}
export function replyNotificationPolicy(kind: ReplyNotificationKind, outcome: ReplyNotificationOutcome, taskReported: boolean, level: TaskNotificationLevel): boolean {
  return kind !== 'watchdog' && (kind !== 'task' || outcome === 'waiting' || !taskReported || level === 'all')
}
export function replyNotificationKind(thread?: { watchdog?: boolean; dispatched?: boolean }): ReplyNotificationKind {
  return thread?.watchdog ? 'watchdog' : thread?.dispatched ? 'task' : 'main'
}
/** Local streams have no report event; an opened task is assumed to report. */
export function localReplyNotificationKind(run: ConversationActivityRun): ReplyNotificationKind {
  return replyNotificationKind(useAccountSyncStore.getState().snapshot?.threads.find(t => t.id === run.thread_id))
}
const windows = new Map<string, { count: number; run: ConversationActivityRun; timer: ReturnType<typeof setTimeout> }>()
export function clearReplyNotificationWindows(): void {
  for (const entry of windows.values()) clearTimeout(entry.timer)
  windows.clear()
}
// Note: Visiting then leaving a conversation also acknowledges its pending updates.
useConversationActivityStore.subscribe((state, previous) => {
  if (state.viewed !== previous.viewed && state.viewed) {
    const entry = windows.get(state.viewed.conversation_id)
    if (entry) entry.count = 0
  }
})
function deliver(run: ConversationActivityRun, body: string): void {
  void showNotification(conversationLabel(run), body).then(result => {
    if (!result.ok) console.warn('Reply notification was not delivered:', result.error)
  })
}
export function notifyReplyOutcome(
  run: ConversationActivityRun, outcome: ReplyNotificationOutcome, detail?: string,
  kind: ReplyNotificationKind = 'main', taskReported = false,
): void {
  if (!replyNotificationPolicy(kind, outcome, taskReported, readTaskNotificationLevel())) return
  if (!readReplyNotificationsEnabled() || isReplyNotificationSuppressed(run)) return
  const pending = windows.get(run.conversation_id)
  if (pending) { pending.count += 1; pending.run = run; return }
  const entry = { count: 0, run, timer: setTimeout(() => {
    windows.delete(run.conversation_id)
    if (entry.count && readReplyNotificationsEnabled() && !isReplyNotificationSuppressed(entry.run)) {
      deliver({ ...entry.run, thread_title: null }, i18n.t('chat:notifications.body.updates', { count: entry.count }))
    }
  }, 20_000) }
  windows.set(run.conversation_id, entry)
  deliver(run, notificationBody(outcome, detail))
}
