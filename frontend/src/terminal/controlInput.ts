/** Standard ASCII control chords; IME strings and escape sequences stay intact. */
export function controlInput(data: string): string {
  if (data === '?') return '\x7f'
  if (data.length !== 1) return data
  const code = data.toUpperCase().charCodeAt(0)
  return code >= 64 && code <= 95 ? String.fromCharCode(code - 64) : data
}
