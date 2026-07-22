import { normalizeTerminalTransportError } from '@/terminal/transport'

/** Log lifecycle diagnostics only; never include terminal input or output. */
export function logTerminalCleanupError(cause: unknown): void {
  const error = normalizeTerminalTransportError(
    cause,
    'terminal.cleanup_failed',
    'Terminal cleanup failed',
  )
  console.error('[terminal] cleanup failed', {
    code: error.code,
    message: error.message,
  })
}
