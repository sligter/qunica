import { beforeEach, describe, expect, it, vi } from 'vitest'

import i18n from '@/i18n'
import {
  notifyReplyOutcome,
  readReplyNotificationsEnabled,
  writeReplyNotificationsEnabled,
} from '@/lib/replyNotifications'
import {
  useConversationActivityStore,
  type ConversationActivityRun,
} from '@/stores/conversationActivityStore'

const mocks = vi.hoisted(() => ({
  showNotification: vi.fn(async () => ({ ok: true as const })),
}))

vi.mock('@/lib/notifications', () => ({
  showNotification: mocks.showNotification,
  notificationsSupported: () => true,
  requestNotificationPermission: async () => 'granted',
  notificationPermission: () => 'granted',
}))

const initialActivity = useConversationActivityStore.getInitialState()

function run(overrides: Partial<ConversationActivityRun> = {}): ConversationActivityRun {
  return {
    id: 'run-1',
    conversation_id: 'group-1',
    thread_id: 'thread-1',
    scope: 'groups',
    conversation_title: 'Platform',
    thread_title: 'Ship the API',
    status: 'running',
    announced: false,
    started_at: '2026-07-15T00:00:00Z',
    updated_at: '2026-07-15T00:00:01Z',
    ...overrides,
  }
}

function setFocus(focused: boolean) {
  vi.spyOn(document, 'hasFocus').mockReturnValue(focused)
}

describe('reply notifications', () => {
  beforeEach(async () => {
    mocks.showNotification.mockReset()
    mocks.showNotification.mockResolvedValue({ ok: true as const })
    localStorage.clear()
    useConversationActivityStore.setState(initialActivity, true)
    setFocus(false)
    await i18n.changeLanguage('en-US')
  })

  it('is on until the user turns it off', () => {
    expect(readReplyNotificationsEnabled()).toBe(true)

    writeReplyNotificationsEnabled(false)
    expect(readReplyNotificationsEnabled()).toBe(false)

    notifyReplyOutcome(run(), 'completed')
    expect(mocks.showNotification).not.toHaveBeenCalled()
  })

  it('names the conversation and its task', () => {
    notifyReplyOutcome(run(), 'completed')

    expect(mocks.showNotification).toHaveBeenCalledWith(
      'Platform · Ship the API',
      'The reply is ready.',
    )
  })

  it('falls back to the container name when the title is unknown', () => {
    notifyReplyOutcome(
      run({ scope: 'direct-chats', conversation_title: null, thread_title: null }),
      'waiting',
    )

    expect(mocks.showNotification).toHaveBeenCalledWith(
      'Chat',
      'The agent is waiting for your input.',
    )
  })

  it('carries the reason a reply failed', () => {
    notifyReplyOutcome(run(), 'failed', 'provider timed out')

    expect(mocks.showNotification).toHaveBeenCalledWith(
      'Platform · Ship the API',
      'The reply failed: provider timed out',
    )
  })

  it('stays quiet about the conversation the user is watching', () => {
    setFocus(true)
    useConversationActivityStore.getState().setViewedConversation('group-1', 'thread-1')

    notifyReplyOutcome(run(), 'completed')

    expect(mocks.showNotification).not.toHaveBeenCalled()
  })

  it('speaks up for another task in the conversation on screen', () => {
    setFocus(true)
    useConversationActivityStore.getState().setViewedConversation('group-1', 'thread-2')

    notifyReplyOutcome(run(), 'completed')

    expect(mocks.showNotification).toHaveBeenCalledTimes(1)
  })

  it('speaks up when the window is in the background', () => {
    setFocus(false)
    useConversationActivityStore.getState().setViewedConversation('group-1', 'thread-1')

    notifyReplyOutcome(run(), 'completed')

    expect(mocks.showNotification).toHaveBeenCalledTimes(1)
  })
})
