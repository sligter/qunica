/**
 * Typed fetch wrapper.
 *
 * Prepends `/api/v1`, attaches the bearer token from `authStore` to every
 * non-auth request, and parses the backend's `{error: {code, message}}`
 * envelope on non-2xx responses.
 */

import type { ApiErrorEnvelope } from '@/types/api'

const BASE = '/api/v1'

export class ApiError extends Error {
  status: number
  code: string

  constructor(status: number, code: string, message: string) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.code = code
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
      return detail
        .map((item) => {
          if (typeof item === 'object' && item !== null && 'msg' in item) {
            return String((item as { msg: unknown }).msg)
          }
          return null
        })
        .filter((message): message is string => message !== null)
        .join('; ') || null
    }
  }
  return null
}

function apiErrorFromResponse(status: number, parsed: unknown, fallbackText: string) {
  if (isApiErrorEnvelope(parsed)) {
    return new ApiError(status, parsed.error.code, parsed.error.message)
  }
  const message = extractErrorMessage(parsed) ?? (fallbackText.trim() || `HTTP ${status}`)
  return new ApiError(status, 'http_error', message)
}

export async function fetchJson<T>(path: string, opts: FetchOptions = {}): Promise<T> {
  const headers: Record<string, string> = {
    Accept: 'application/json',
  }
  if (opts.body !== undefined) {
    headers['Content-Type'] = 'application/json'
  }
  if (opts.token) {
    headers['Authorization'] = `Bearer ${opts.token}`
  }

  const res = await fetch(`${BASE}${path}`, {
    method: opts.method ?? 'GET',
    headers,
    body: opts.body !== undefined ? JSON.stringify(opts.body) : undefined,
    signal: opts.signal,
  })

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
  opts: { token?: string | null; method?: string } = {},
): Promise<T> {
  const headers: Record<string, string> = {
    Accept: 'application/json',
  }
  if (opts.token) {
    headers['Authorization'] = `Bearer ${opts.token}`
  }

  const res = await fetch(`${BASE}${path}`, {
    method: opts.method ?? 'POST',
    headers,
    body: formData,
  })

  if (res.status === 204) {
    return undefined as T
  }

  let parsed: unknown = null
  const text = await res.text()
  if (text) {
    try {
      parsed = JSON.parse(text)
    } catch {
      // fall through
    }
  }

  if (!res.ok) {
    throw apiErrorFromResponse(res.status, parsed, text)
  }

  return parsed as T
}
