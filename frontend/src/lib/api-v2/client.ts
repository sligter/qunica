/**
 * Typed API v2 fetch wrapper.
 *
 * Prepends `/api/v2`, attaches an explicit bearer token, and parses backend
 * error envelopes on non-2xx responses.
 */

import { apiUrl } from '@/lib/runtime'

import { fetchWithRetry } from './retry'
import type { ApiErrorEnvelope } from './types'

const BASE = '/api/v2'

export class ApiError extends Error {
  status: number
  code: string
  details?: unknown

  constructor(status: number, code: string, message: string, details?: unknown) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.code = code
    if (details !== undefined) {
      this.details = details
    }
    Object.setPrototypeOf(this, ApiError.prototype)
  }
}

interface FetchOptions {
  method?: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE'
  body?: unknown
  token?: string | null
  signal?: AbortSignal
}

function isApiErrorEnvelope(value: unknown): value is ApiErrorEnvelope {
  if (typeof value !== 'object' || value === null || !('error' in value)) {
    return false
  }
  const error = (value as { error: unknown }).error
  return (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    'message' in error &&
    typeof (error as { code: unknown }).code === 'string' &&
    typeof (error as { message: unknown }).message === 'string'
  )
}

function extractErrorMessage(value: unknown): string | null {
  if (isApiErrorEnvelope(value)) {
    return value.error.message
  }
  if (typeof value === 'object' && value !== null && 'detail' in value) {
    const detail = (value as { detail: unknown }).detail
    if (typeof detail === 'string') {
      return detail
    }
    if (Array.isArray(detail)) {
      return (
        detail
          .map((item) => {
            if (typeof item === 'object' && item !== null && 'msg' in item) {
              return String((item as { msg: unknown }).msg)
            }
            return null
          })
          .filter((message): message is string => message !== null)
          .join('; ') || null
      )
    }
  }
  return null
}

function apiErrorFromResponse(status: number, parsed: unknown, fallbackText: string) {
  if (isApiErrorEnvelope(parsed)) {
    return new ApiError(
      status,
      parsed.error.code,
      parsed.error.message,
      parsed.error.details,
    )
  }
  const message = extractErrorMessage(parsed) ?? (fallbackText.trim() || `HTTP ${status}`)
  return new ApiError(status, 'http_error', message)
}

export async function fetchJson<T>(path: string, opts: FetchOptions = {}): Promise<T> {
  const method = opts.method ?? 'GET'
  const headers: Record<string, string> = {
    Accept: 'application/json',
  }
  if (opts.body !== undefined) {
    headers['Content-Type'] = 'application/json'
  }
  if (opts.token) {
    headers.Authorization = `Bearer ${opts.token}`
  }

  const res = await fetchWithRetry(apiUrl(`${BASE}${path}`), {
    method,
    headers,
    body: opts.body !== undefined ? JSON.stringify(opts.body) : undefined,
    signal: opts.signal,
  }, method === 'GET' || method === 'DELETE' || path.endsWith('/cancel'))

  if (res.status === 204) {
    return undefined as T
  }

  let parsed: unknown = null
  const text = await res.text()
  if (text) {
    try {
      parsed = JSON.parse(text)
    } catch {
      // fall through; non-JSON response
    }
  }

  if (!res.ok) {
    throw apiErrorFromResponse(res.status, parsed, text)
  }

  return parsed as T
}

export async function fetchFormData<T>(
  path: string,
  formData: FormData,
  opts: { token?: string | null; method?: FetchOptions['method']; signal?: AbortSignal } = {},
): Promise<T> {
  const headers: Record<string, string> = {
    Accept: 'application/json',
  }
  if (opts.token) {
    headers.Authorization = `Bearer ${opts.token}`
  }

  const method = opts.method ?? 'POST'
  const res = await fetchWithRetry(apiUrl(`${BASE}${path}`), {
    method,
    headers,
    body: formData,
    signal: opts.signal,
  }, method === 'GET' || method === 'DELETE' || path.endsWith('/cancel'))

  if (res.status === 204) {
    return undefined as T
  }

  let parsed: unknown = null
  const text = await res.text()
  if (text) {
    try {
      parsed = JSON.parse(text)
    } catch {
      // fall through; non-JSON response
    }
  }

  if (!res.ok) {
    throw apiErrorFromResponse(res.status, parsed, text)
  }

  return parsed as T
}
