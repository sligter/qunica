import type {
  CreateTerminalRequest,
  TerminalDescriptor,
  TerminalEvent,
} from './types'

export interface TerminalTransport {
  create(
    request: CreateTerminalRequest,
    onEvent: (event: TerminalEvent) => void,
  ): Promise<TerminalDescriptor>
  write(conversationId: string, sessionId: string, data: string): Promise<void>
  resize(
    conversationId: string,
    sessionId: string,
    cols: number,
    rows: number,
  ): Promise<void>
  close(conversationId: string, sessionId: string): Promise<void>
  closeAll(): Promise<void>
}

export class TerminalTransportError extends Error {
  readonly code: string

  constructor(code: string, message: string) {
    super(message)
    this.name = 'TerminalTransportError'
    this.code = code
  }
}

const DESKTOP_REQUIRED_CODE = 'terminal.desktop_required'
const DESKTOP_REQUIRED_MESSAGE = 'Terminal is available only in the desktop app.'

function desktopRequired(): Promise<never> {
  return Promise.reject(
    new TerminalTransportError(DESKTOP_REQUIRED_CODE, DESKTOP_REQUIRED_MESSAGE),
  )
}

export function createUnavailableTerminalTransport(): TerminalTransport {
  return {
    create: desktopRequired,
    write: desktopRequired,
    resize: desktopRequired,
    close: desktopRequired,
    closeAll: desktopRequired,
  }
}

export const unavailableTerminalTransport = createUnavailableTerminalTransport()

export function normalizeTerminalTransportError(
  cause: unknown,
  fallbackCode = 'terminal.command_failed',
  fallbackMessage = 'Terminal command failed',
): TerminalTransportError {
  if (cause instanceof TerminalTransportError) {
    return cause
  }

  if (typeof cause === 'object' && cause !== null) {
    const candidate = cause as Record<string, unknown>
    if (typeof candidate.code === 'string' && typeof candidate.message === 'string') {
      return new TerminalTransportError(candidate.code, candidate.message)
    }
  }

  if (cause instanceof Error && cause.message) {
    return new TerminalTransportError(fallbackCode, cause.message)
  }

  if (typeof cause === 'string' && cause) {
    return new TerminalTransportError(fallbackCode, cause)
  }

  return new TerminalTransportError(fallbackCode, fallbackMessage)
}
