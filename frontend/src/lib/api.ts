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
  return (
    typeof value === 'object' &&
    value !== null &&
    'error' in value &&
    typeof (value as { error: unknown }).error === 'object'
  )
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
    if (isApiErrorEnvelope(parsed)) {
      throw new ApiError(res.status, parsed.error.code, parsed.error.message)
    }
    throw new ApiError(res.status, 'http_error', `HTTP ${res.status}`)
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
    if (isApiErrorEnvelope(parsed)) {
      throw new ApiError(res.status, parsed.error.code, parsed.error.message)
    }
    throw new ApiError(res.status, 'http_error', `HTTP ${res.status}`)
  }

  return parsed as T
}
