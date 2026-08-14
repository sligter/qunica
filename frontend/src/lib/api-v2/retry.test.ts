import { afterEach, describe, expect, it, vi } from 'vitest'

import {
  fetchWithRetry,
  isRetryableHttpStatus,
  retryDelayMs,
} from './retry'

afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('API retry policy', () => {
  it('uses capped exponential backoff and only the recoverable HTTP statuses', () => {
    expect([1, 2, 3, 4, 5, 6, 7, 10].map((attempt) => retryDelayMs(attempt, () => 0.5)))
      .toEqual([500, 1_000, 2_000, 4_000, 8_000, 16_000, 30_000, 30_000])
    expect([408, 425, 429, 500, 502, 503, 504].every(isRetryableHttpStatus)).toBe(true)
    expect([400, 401, 403, 404, 422].some(isRetryableHttpStatus)).toBe(false)
  })

  it('retries network failures for writes but not an HTTP response from a write', async () => {
    vi.useFakeTimers()
    vi.spyOn(Math, 'random').mockReturnValue(0.5)
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockRejectedValueOnce(new TypeError('fetch failed'))
      .mockResolvedValueOnce(new Response(null, { status: 204 }))
      .mockResolvedValueOnce(new Response(null, { status: 503 }))
    vi.stubGlobal('fetch', fetchMock)

    const recovered = fetchWithRetry('/write', { method: 'POST' }, false)
    await vi.runAllTimersAsync()
    expect((await recovered).status).toBe(204)

    expect((await fetchWithRetry('/write', { method: 'POST' }, false)).status).toBe(503)
    expect(fetchMock).toHaveBeenCalledTimes(3)
  })

  it('retries a recoverable HTTP response for an idempotent request', async () => {
    vi.useFakeTimers()
    vi.spyOn(Math, 'random').mockReturnValue(0.5)
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(new Response(null, { status: 503 }))
      .mockResolvedValueOnce(new Response('{}', { status: 200 }))
    vi.stubGlobal('fetch', fetchMock)

    const recovered = fetchWithRetry('/read', { method: 'GET' }, true)
    await vi.runAllTimersAsync()

    expect((await recovered).status).toBe(200)
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })

  it('does not retry an aborted request', async () => {
    const fetchMock = vi.fn<typeof fetch>().mockRejectedValue(
      new DOMException('aborted', 'AbortError'),
    )
    vi.stubGlobal('fetch', fetchMock)

    await expect(
      fetchWithRetry('/read', { signal: new AbortController().signal }, true),
    ).rejects.toMatchObject({ name: 'AbortError' })
    expect(fetchMock).toHaveBeenCalledOnce()
  })
})
