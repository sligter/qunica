export type LocalizedError =
  | { kind: 'translation'; key: string }
  | { kind: 'detail'; key: string; message: string }

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
