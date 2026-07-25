import { ApiError } from '@/lib/api-v2/client'

export type LocalizedError =
  | { kind: 'translation'; key: string }
  | { kind: 'detail'; key: string; message: string }

export type WorkspaceErrorMessageKey =
  | 'workspace.errorMessages.invalidInput'
  | 'workspace.errorMessages.notFound'
  | 'workspace.errorMessages.conflict'
  | 'workspace.errorMessages.permissionDenied'
  | 'workspace.errorMessages.unauthorized'
  | 'workspace.errorMessages.server'
  | 'workspace.errorMessages.network'
  | 'workspace.errorMessages.unexpected'

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message.toLowerCase()
  return typeof error === 'string' ? error.toLowerCase() : ''
}

export function workspaceErrorMessageKey(error: unknown): WorkspaceErrorMessageKey {
  if (error instanceof ApiError) {
    if (error.status === 409 || error.code === 'conflict') {
      return 'workspace.errorMessages.conflict'
    }
    if (error.status === 404 || error.code === 'not_found') {
      return 'workspace.errorMessages.notFound'
    }
    if (error.status === 403 || error.code === 'permission_denied') {
      return 'workspace.errorMessages.permissionDenied'
    }
    if (error.status === 401 || error.code === 'unauthorized') {
      return 'workspace.errorMessages.unauthorized'
    }
    if (error.status === 400 || error.code === 'invalid_input') {
      return 'workspace.errorMessages.invalidInput'
    }
    if (error.status >= 500 || error.code === 'internal_error') {
      return 'workspace.errorMessages.server'
    }
  }

  const message = errorMessage(error)
  if (/changed since|changed during|\bconflict\b/.test(message)) {
    return 'workspace.errorMessages.conflict'
  }
  if (/not found|no longer (?:exists|available)/.test(message)) {
    return 'workspace.errorMessages.notFound'
  }
  if (/permission|forbidden|belongs to another user/.test(message)) {
    return 'workspace.errorMessages.permissionDenied'
  }
  if (/unauthori|authentication|sign.?in/.test(message)) {
    return 'workspace.errorMessages.unauthorized'
  }
  if (/failed to fetch|network|offline/.test(message) || error instanceof TypeError) {
    return 'workspace.errorMessages.network'
  }
  if (/invalid|required|must |not a (?:file|directory)|too large|utf-?8|nul byte|escapes the/.test(message)) {
    return 'workspace.errorMessages.invalidInput'
  }
  if (/database|internal|failed to (?:read|write|replace|create|flush|preserve)/.test(message)) {
    return 'workspace.errorMessages.server'
  }
  return 'workspace.errorMessages.unexpected'
}

export function translatedError(key: string): LocalizedError {
  return { kind: 'translation', key }
}

export function messageError(
  message: string,
  key = 'common:errors.detail',
): LocalizedError {
  return { kind: 'detail', key, message }
}

export function localizedErrorText(
  error: LocalizedError | null,
  translate: (key: string, options?: Record<string, unknown>) => string,
): string | null {
  if (!error) return null
  return error.kind === 'translation'
    ? translate(error.key)
    : translate(error.key, { message: error.message })
}
