import type { FetchEventSourceInit } from '@microsoft/fetch-event-source'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({ fetchEventSource: vi.fn() }))

vi.mock('@microsoft/fetch-event-source', () => ({
  fetchEventSource: mocks.fetchEventSource,
}))

import { openApiV2SseStream, SseRetryExhaustedError } from './sse'

describe('API v2 SSE retry policy', () => {
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
