export type LocalizedError =
  | { kind: 'translation'; key: string }
  | { kind: 'message'; message: string }

export function translatedError(key: string): LocalizedError {
  return { kind: 'translation', key }
}

export function messageError(message: string): LocalizedError {
  return { kind: 'message', message }
}

export function localizedErrorText(
  error: LocalizedError | null,
  translate: (key: string) => string,
): string | null {
  if (!error) return null
  return error.kind === 'translation' ? translate(error.key) : error.message
}
