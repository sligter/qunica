import type { ReactNode } from 'react'
import { act, renderHook } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useSendMessageStream } from '@/hooks/useSendMessageStream'
import i18n from '@/i18n'
import type { ApiV2SseHandlers } from '@/lib/api-v2/sse'
import { useAuthStore } from '@/stores/authStore'
import {
  selectConversationStatus,
  selectThreadStatus,
  useConversationActivityStore,
} from '@/stores/conversationActivityStore'
import { useMessageStore } from '@/stores/messageStore'

const mocks = vi.hoisted(() => ({
  streams: [] as Array<{ handlers: ApiV2SseHandlers }>,
  fetchJson: vi.fn(),
  showNotification: vi.fn(async () => ({ ok: true as const })),
}))

vi.mock('@/lib/api-v2/client', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/lib/api-v2/client')>()
  return { ...original, fetchJson: mocks.fetchJson }
})

vi.mock('@/lib/api-v2/sse', () => ({
  openApiV2SseStream: (options: { handlers: ApiV2SseHandlers }) => {
    const abort = vi.fn()
    mocks.streams.push({ handlers: options.handlers })
    return { abort } as unknown as AbortController
  },
}))

vi.mock('@/lib/notifications', () => ({
  showNotification: mocks.showNotification,
  notificationsSupported: () => true,
  requestNotificationPermission: async () => 'granted',
  notificationPermission: () => 'granted',
}))

const initialMessages = useMessageStore.getInitialState()
const initialActivity = useConversationActivityStore.getInitialState()

function wrapper(queryClient: QueryClient) {
  return function TestWrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  }
}

function emit(handlers: ApiV2SseHandlers, kind: string, payload: unknown, seq: number) {
  act(() =>
    handlers.onEvent({
      stream_id: 'stream-1',
      seq,
      event_id: `event-${seq}`,
      kind,
      payload,
    } as never),
  )
}

function sendMessage() {
  const queryClient = new QueryClient()
  const hook = renderHook(
    () => useSendMessageStream('group-1', { threadId: 'thread-1' }),
    { wrapper: wrapper(queryClient) },
  )
  act(() => {
    void hook.result.current.send('hello').catch(() => undefined)
  })
  const handlers = mocks.streams[0]?.handlers
  if (!handlers) throw new Error('the send did not open a stream')
  return handlers
}

function conversationStatus() {
  return selectConversationStatus(useConversationActivityStore.getState(), 'group-1')
}

function threadStatus() {
  return selectThreadStatus(useConversationActivityStore.getState(), 'group-1', 'thread-1')
}

describe('useSendMessageStream conversation activity', () => {
  beforeEach(async () => {
    vi.useRealTimers()
    mocks.streams.length = 0
    mocks.fetchJson.mockReset()
    mocks.showNotification.mockReset()
    mocks.showNotification.mockResolvedValue({ ok: true as const })
    localStorage.clear()
    useMessageStore.setState(initialMessages, true)
    useConversationActivityStore.setState(initialActivity, true)
    useAuthStore.setState({ token: 'token-1', user: null, hydrated: true })
    vi.spyOn(document, 'hasFocus').mockReturnValue(false)
    await i18n.changeLanguage('en-US')
    useConversationActivityStore.getState().registerConversationTitles('group-1', 'thread-1', {
      conversation: 'Platform',
      thread: 'Ship the API',
    })
  })

  it('marks the conversation busy for the life of the stream', () => {
    const handlers = sendMessage()

    expect(conversationStatus()).toBe('running')
    expect(threadStatus()).toBe('running')

    emit(handlers, 'done', {}, 1)

    expect(conversationStatus()).toBeNull()
  })

  it('announces a finished reply once', () => {
    const handlers = sendMessage()

    emit(handlers, 'done', {}, 1)

    expect(mocks.showNotification).toHaveBeenCalledTimes(1)
    expect(mocks.showNotification).toHaveBeenCalledWith(
      'Platform · Ship the API',
      'The reply is ready.',
    )
  })

  it('announces a pause instead of the close that carried it', () => {
    const handlers = sendMessage()

    emit(handlers, 'waiting_for_user', { message: 'Which branch?' }, 1)
    emit(handlers, 'done', {}, 2)

    expect(mocks.showNotification).toHaveBeenCalledTimes(1)
    expect(mocks.showNotification).toHaveBeenCalledWith(
      'Platform · Ship the API',
      'The agent is waiting for your input.',
    )
    expect(threadStatus()).toBe('waiting')
  })

  it('reports the reason a stream failed', () => {
    const handlers = sendMessage()

    emit(handlers, 'error', { message: 'provider timed out' }, 1)
    emit(handlers, 'done', {}, 2)

    expect(mocks.showNotification).toHaveBeenCalledWith(
      'Platform · Ship the API',
      'The reply failed: provider timed out',
    )
    expect(threadStatus()).toBe('failed')
  })

  it('stays quiet about the conversation on screen', () => {
    vi.spyOn(document, 'hasFocus').mockReturnValue(true)
    useConversationActivityStore.getState().setViewedConversation('group-1', 'thread-1')
    const handlers = sendMessage()

    emit(handlers, 'done', {}, 1)

    expect(mocks.showNotification).not.toHaveBeenCalled()
  })

  it('announces a reply that lands after the user navigated away', () => {
    vi.spyOn(document, 'hasFocus').mockReturnValue(true)
    const activity = useConversationActivityStore.getState()
    activity.setViewedConversation('group-1', 'thread-1')
    const handlers = sendMessage()

    // Leaving the conversation is what the chat view does on unmount.
    activity.clearViewedConversation('group-1')
    emit(handlers, 'done', {}, 1)

    expect(mocks.showNotification).toHaveBeenCalledWith(
      'Platform · Ship the API',
      'The reply is ready.',
    )
  })

  it('says nothing when the user turned notifications off', () => {
    localStorage.setItem('ag-swarmer:notifications:reply-finished', 'false')
    const handlers = sendMessage()

    emit(handlers, 'done', {}, 1)

    expect(mocks.showNotification).not.toHaveBeenCalled()
    expect(conversationStatus()).toBeNull()
  })
})
