import type { FetchEventSourceInit } from '@microsoft/fetch-event-source'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({ fetchEventSource: vi.fn() }))

vi.mock('@microsoft/fetch-event-source', () => ({
  fetchEventSource: mocks.fetchEventSource,
}))

import { openApiV2SseStream, SseRetryExhaustedError } from './sse'
import { useAuthStore } from '@/stores/authStore'

describe('API v2 SSE retry policy', () => {
  it('recovers through GET with the applied cursor, deduplicates, and ignores superseded callbacks', async () => {
    useAuthStore.setState({ token: 'token-1' })
    const fetch = vi.fn().mockResolvedValue(new Response(null, { status: 200 }))
    vi.stubGlobal('fetch', fetch)
    const onEvent = vi.fn()
    const onRecover = vi.fn().mockResolvedValue(true)
    const onDisconnect = vi.fn()
    const onError = vi.fn()
    const ctrl = openApiV2SseStream({
      url: '/send', body: { content: 'once' }, token: 'token-1',
      replayUrl: () => '/replay', handlers: { onEvent, onRecover, onDisconnect, onError },
    })
    try {
      const first = init
      await first.fetch?.('/send', { method: 'POST', body: '{}', headers: { Authorization: 'Bearer token-1' } })
      const frame = (seq: number) => ({ id: 'stream:' + seq, event: '', retry: undefined,
        data: JSON.stringify({ stream_id: 'stream', event_id: 'stream:' + seq, seq, kind: 'token', payload: { delta: 'a' } }) })
      first.onmessage?.(frame(1))
      first.onmessage?.(frame(1))
      expect(onEvent).toHaveBeenCalledOnce()
      onEvent.mockImplementationOnce(() => { throw new Error('apply failed') })
      expect(() => first.onmessage?.(frame(2))).toThrow('apply failed')
      await first.fetch?.('/send', { method: 'POST', body: '{}', headers: { 'last-event-id': 'stream:2' } })
      expect(fetch.mock.calls[1][0]).toBe('/replay')
      expect(fetch.mock.calls[1][1].method).toBe('GET')
      expect(fetch.mock.calls[1][1].body).toBeUndefined()
      expect(fetch.mock.calls[1][1].headers.get('last-event-id')).toBe('stream:1')
      for (let attempt = 0; attempt < 10; attempt++) first.onerror?.(new TypeError('offline'))
      expect(() => first.onerror?.(new TypeError('offline'))).toThrow(SseRetryExhaustedError)
      expect(onDisconnect).toHaveBeenCalledOnce()
      expect(onError).not.toHaveBeenCalled()
      window.dispatchEvent(new Event('online'))
      document.dispatchEvent(new Event('visibilitychange'))
      await vi.waitFor(() => expect(mocks.fetchEventSource).toHaveBeenCalledTimes(2))
      expect(onRecover).toHaveBeenCalledOnce()
      expect(first.signal?.aborted).toBe(true)
      first.onmessage?.(frame(3))
      expect(onEvent).toHaveBeenCalledTimes(2)
      init.onmessage?.(frame(2))
      expect(onEvent).toHaveBeenCalledTimes(3)
      ctrl.abort()
      window.dispatchEvent(new Event('online'))
      expect(onRecover).toHaveBeenCalledOnce()
    } finally {
      ctrl.abort()
      vi.unstubAllGlobals()
      useAuthStore.setState({ token: null })
    }
  })

  let init: FetchEventSourceInit

  beforeEach(() => {
    vi.restoreAllMocks()
    mocks.fetchEventSource.mockReset().mockImplementation((_input, options) => {
      init = options as FetchEventSourceInit
      return Promise.resolve()
    })
  })

  function open() {
    const handlers = {
      onEvent: vi.fn(),
      onOpen: vi.fn(),
      onRetry: vi.fn(),
      onError: vi.fn(),
      onClose: vi.fn(),
    }
    openApiV2SseStream({
      url: '/api/v2/groups/group-1/messages/stream',
      body: {},
      token: 'token-1',
      lastEventId: '00000000-0000-0000-0000-000000000000:4',
      handlers,
    })
    return handlers
  }

  it('refreshes a resume without an identity instead of posting its approval twice', async () => {
    const fetch = vi.fn().mockResolvedValue(new Response(null, { status: 200 }))
    vi.stubGlobal('fetch', fetch)
    const onRecover = vi.fn().mockResolvedValue(true)
    const ctrl = openApiV2SseStream({
      url: '/resume', body: { approval: { approved: true } }, token: 'token-1',
      replayUrl: () => null, handlers: { onEvent: vi.fn(), onRecover },
    })
    try {
      await init.fetch?.('/resume', { method: 'POST', body: '{}' })
      await expect(init.fetch?.('/resume', { method: 'POST', body: '{}' }))
        .rejects.toThrow('execution was not repeated')
      expect(onRecover).toHaveBeenCalledOnce()
      expect(fetch).toHaveBeenCalledOnce()
    } finally {
      ctrl.abort()
      vi.unstubAllGlobals()
    }
  })

  it('closes when the server completed in the background without replaying or executing again', async () => {
    const onClose = vi.fn()
    const onRecover = vi.fn().mockResolvedValue(false)
    const ctrl = openApiV2SseStream({
      url: '/send', body: {}, token: 'token-1', replayUrl: () => '/replay',
      handlers: { onEvent: vi.fn(), onRecover, onClose },
    })
    const first = init
    window.dispatchEvent(new Event('online'))
    await vi.waitFor(() => expect(onClose).toHaveBeenCalledOnce())
    expect(first.signal?.aborted).toBe(true)
    window.dispatchEvent(new Event('online'))
    expect(onRecover).toHaveBeenCalledOnce()
    expect(mocks.fetchEventSource).toHaveBeenCalledOnce()
    ctrl.abort()
  })

  it('does not reopen after cancellation during an asynchronous state refresh', async () => {
    let finishRecovery!: (value: boolean) => void
    const onClose = vi.fn()
    const ctrl = openApiV2SseStream({
      url: '/send', body: {}, token: 'token-1', replayUrl: () => '/replay',
      handlers: { onEvent: vi.fn(), onClose,
        onRecover: () => new Promise(resolve => { finishRecovery = resolve }) },
    })
    window.dispatchEvent(new Event('online'))
    ctrl.abort()
    finishRecovery(true)
    await Promise.resolve()
    expect(mocks.fetchEventSource).toHaveBeenCalledOnce()
    expect(onClose).not.toHaveBeenCalled()
  })

  it('retries ten times with capped exponential backoff, then reports exhaustion', () => {
    vi.spyOn(Math, 'random').mockReturnValue(0.5)
    const handlers = open()
    const failure = new TypeError('fetch failed')

    const delays = Array.from({ length: 10 }, () => init.onerror?.(failure))
    expect(delays).toEqual([
      500,
      1_000,
      2_000,
      4_000,
      8_000,
      16_000,
      30_000,
      30_000,
      30_000,
      30_000,
    ])
    expect(handlers.onRetry).toHaveBeenCalledTimes(10)
    expect(() => init.onerror?.(failure)).toThrow(SseRetryExhaustedError)
    expect(handlers.onError).toHaveBeenCalledWith(expect.any(SseRetryExhaustedError))
  })

  it('retries recoverable HTTP failures, fails fast on other HTTP errors, and keeps the cursor', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0.5)
    const handlers = open()
    expect((init.headers as Record<string, string>)['last-event-id'])
      .toBe('00000000-0000-0000-0000-000000000000:4')

    const retryable = await init.onopen?.(new Response(null, { status: 503 }))
      .catch((error: unknown) => error)
    expect(init.onerror?.(retryable)).toBe(500)
    await init.onopen?.(new Response(null, { status: 200 }))
    expect(handlers.onOpen).toHaveBeenCalledOnce()

    const fatal = await init.onopen?.(new Response(null, { status: 401 }))
      .catch((error: unknown) => error)
    expect(() => init.onerror?.(fatal)).toThrow('SSE open failed: 401')
    expect(handlers.onError).toHaveBeenCalledWith(fatal)
  })

  it('resets consecutive failures after receiving a valid event', () => {
    vi.spyOn(Math, 'random').mockReturnValue(0.5)
    const handlers = open()
    const failure = new TypeError('fetch failed')
    expect(init.onerror?.(failure)).toBe(500)
    expect(init.onerror?.(failure)).toBe(1_000)

    init.onmessage?.({
      id: '00000000-0000-0000-0000-000000000000:5',
      event: '',
      retry: undefined,
      data: JSON.stringify({
        stream_id: '00000000-0000-0000-0000-000000000000',
        seq: 5,
        event_id: '00000000-0000-0000-0000-000000000000:5',
        kind: 'token',
        payload: { delta: 'ok' },
      }),
    })

    expect(handlers.onEvent).toHaveBeenCalledOnce()
    expect(init.onerror?.(failure)).toBe(500)
  })

  it('ignores empty keep-alive events', () => {
    const handlers = open()

    expect(() => init.onmessage?.({ id: '', event: '', retry: undefined, data: '' }))
      .not.toThrow()
    expect(handlers.onEvent).not.toHaveBeenCalled()
  })

  it('retries when applying a valid event throws', () => {
    vi.spyOn(Math, 'random').mockReturnValue(0.5)
    const handlers = open()
    handlers.onEvent.mockImplementation(() => {
      throw new Error('Minified React error #185')
    })

    const applyEvent = () => {
      try {
        init.onmessage?.({
          id: '00000000-0000-0000-0000-000000000000:5',
          event: '',
          retry: undefined,
          data: JSON.stringify({
            stream_id: '00000000-0000-0000-0000-000000000000',
            seq: 5,
            event_id: '00000000-0000-0000-0000-000000000000:5',
            kind: 'token',
            payload: { delta: 'ok' },
          }),
        })
      } catch (error) {
        return error
      }
    }

    expect(init.onerror?.(applyEvent())).toBe(500)
    expect(init.onerror?.(applyEvent())).toBe(1_000)
    expect(handlers.onRetry).toHaveBeenNthCalledWith(1, 1, 500)
    expect(handlers.onRetry).toHaveBeenNthCalledWith(2, 2, 1_000)
    expect(handlers.onError).not.toHaveBeenCalled()
  })
})
