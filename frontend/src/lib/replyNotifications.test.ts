import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import i18n from '@/i18n'
import { useAccountSyncStore } from '@/stores/accountSyncStore'
import {
  notifyReplyOutcome,
  clearReplyNotificationWindows,
  readTaskNotificationLevel,
  writeTaskNotificationLevel,
  replyNotificationPolicy,
  localReplyNotificationKind,
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
    clearReplyNotificationWindows()
    vi.useRealTimers()
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

for (const kind of ['main', 'task', 'watchdog'] as const) {
  for (const outcome of ['completed', 'failed', 'waiting'] as const) {
    for (const reported of [true, false]) {
      for (const level of ['summary', 'all'] as const) {
        it(`policy ${kind}/${outcome}/${reported}/${level}`, () => {
          const expected = kind === 'main' || (kind === 'task' && (outcome === 'waiting' || !reported || level === 'all'))
          expect(replyNotificationPolicy(kind, outcome, reported, level)).toBe(expected)
        })
      }
    }
  }
}
describe('notification windows and preferences', () => {
  beforeEach(async () => {
    clearReplyNotificationWindows()
    vi.useFakeTimers()
    mocks.showNotification.mockClear()
    localStorage.clear()
    useConversationActivityStore.setState(initialActivity, true)
    setFocus(false)
    await i18n.changeLanguage('en-US')
  })
  it('sends the first immediately and combines subsequent updates after 20 seconds', () => {
    notifyReplyOutcome(run(), 'completed')
    notifyReplyOutcome(run(), 'waiting')
    notifyReplyOutcome(run(), 'failed')
    expect(mocks.showNotification).toHaveBeenCalledTimes(1)
    vi.advanceTimersByTime(19_999)
    expect(mocks.showNotification).toHaveBeenCalledTimes(1)
    vi.advanceTimersByTime(1)
    expect(mocks.showNotification).toHaveBeenLastCalledWith('Platform', '2 updates')
    expect(mocks.showNotification).toHaveBeenCalledTimes(2)
  })
  it('keeps separate conversations independent and opens a fresh window after expiry', () => {
    notifyReplyOutcome(run(), 'completed')
    notifyReplyOutcome(run({ conversation_id: 'other' }), 'completed')
    expect(mocks.showNotification).toHaveBeenCalledTimes(2)
    vi.advanceTimersByTime(20_000)
    notifyReplyOutcome(run(), 'completed')
    expect(mocks.showNotification).toHaveBeenCalledTimes(3)
  })
  it('discards accumulated updates when the user visits then leaves', () => {
    notifyReplyOutcome(run(), 'completed')
    notifyReplyOutcome(run(), 'waiting')
    useConversationActivityStore.getState().setViewedConversation('group-1', 'thread-1')
    useConversationActivityStore.getState().setViewedConversation('other', null)
    vi.advanceTimersByTime(20_000)
    expect(mocks.showNotification).toHaveBeenCalledTimes(1)
  })
  it('defaults to summary and persists both levels', () => {
    expect(readTaskNotificationLevel()).toBe('summary')
    writeTaskNotificationLevel('all')
    expect(readTaskNotificationLevel()).toBe('all')
    writeTaskNotificationLevel('summary')
    expect(readTaskNotificationLevel()).toBe('summary')
    localStorage.setItem('qunica:notifications:task-updates', 'invalid')
    expect(readTaskNotificationLevel()).toBe('summary')
  })
  it('uses summary when storage is unavailable', () => {
    const spy = vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new Error('unavailable') })
    expect(readTaskNotificationLevel()).toBe('summary')
    spy.mockRestore()
  })
})

afterEach(() => { clearReplyNotificationWindows(); vi.useRealTimers() })

it('reads dispatched and watchdog flags from the local account snapshot', () => {
  const threads = [
    { id: 'task', group_id: 'group-1', title: '', status: 'active', dispatched: true },
    { id: 'check', group_id: 'group-1', title: '', status: 'active', watchdog: true, dispatched: true },
  ]
  useAccountSyncStore.setState({ snapshot: { server_id: 'server', cursor: '1', groups: [], threads, active_jobs: [], pending_approvals: [] } })
  expect(localReplyNotificationKind(run({ thread_id: 'task' }))).toBe('task')
  expect(localReplyNotificationKind(run({ thread_id: 'check' }))).toBe('watchdog')
  expect(localReplyNotificationKind(run({ thread_id: 'unknown' }))).toBe('main')
  useAccountSyncStore.setState(useAccountSyncStore.getInitialState(), true)
})
