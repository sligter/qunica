import { authFetch } from '@/lib/authFetch'

export const MAX_RETRY_ATTEMPTS = 10

const BASE_RETRY_MS = 500
const MAX_RETRY_MS = 30_000
const RETRYABLE_HTTP_STATUSES = new Set([408, 425, 429, 500, 502, 503, 504])

export interface RetryState {
  attempt: number
  delayMs: number
}

export function isAbortError(error: unknown): boolean {
  return (
    typeof error === 'object' &&
    error !== null &&
    'name' in error &&
    error.name === 'AbortError'
  )
}

export function isNetworkError(error: unknown): boolean {
  return (
    error instanceof TypeError ||
    (typeof error === 'object' &&
      error !== null &&
      'name' in error &&
      error.name === 'NetworkError')
  )
}

export function isRetryableHttpStatus(status: number): boolean {
  return RETRYABLE_HTTP_STATUSES.has(status)
}

export function retryDelayMs(attempt: number, random = Math.random): number {
  const exponential = Math.min(BASE_RETRY_MS * 2 ** Math.max(0, attempt - 1), MAX_RETRY_MS)
  return Math.min(Math.round(exponential * (0.8 + random() * 0.4)), MAX_RETRY_MS)
}

async function wait(delayMs: number, signal?: AbortSignal): Promise<void> {
  if (signal?.aborted) throw signal.reason

  await new Promise<void>((resolve, reject) => {
    const onAbort = () => {
      clearTimeout(timer)
      reject(signal?.reason ?? new DOMException('The operation was aborted', 'AbortError'))
    }
    const timer = setTimeout(() => {
      signal?.removeEventListener('abort', onAbort)
      resolve()
    }, delayMs)
    signal?.addEventListener('abort', onAbort, { once: true })
  })
}

/** Retry fetch failures before a response; retry HTTP responses only when the caller is idempotent. */
export async function fetchWithRetry(
  input: RequestInfo | URL,
  init: RequestInit,
  retryHttpResponses: boolean,
): Promise<Response> {
  let attempts = 0

  for (;;) {
    let response: Response
    try {
      response = await authFetch(input, init)
    } catch (error) {
      if (
        init.signal?.aborted ||
        isAbortError(error) ||
        !isNetworkError(error) ||
        attempts >= MAX_RETRY_ATTEMPTS
      ) {
        throw error
      }
      attempts += 1
      await wait(retryDelayMs(attempts), init.signal ?? undefined)
      continue
    }

    if (
      !retryHttpResponses ||
      !isRetryableHttpStatus(response.status) ||
      attempts >= MAX_RETRY_ATTEMPTS
    ) {
      return response
    }

    attempts += 1
    await response.body?.cancel().catch(() => undefined)
    await wait(retryDelayMs(attempts), init.signal ?? undefined)
  }
}
