import { Channel, invoke } from '@tauri-apps/api/core'

import {
  decodeBase64Bytes,
  toWellFormedUnicode,
  utf8InputChunks,
} from './encoding'
import {
  normalizeTerminalTransportError,
  type TerminalTransport,
} from './transport'
import type {
  TerminalDescriptor,
  TerminalEvent,
} from './types'

// Re-exported so the desktop transport's own tests keep a single import site.
export { decodeBase64Bytes }

type WireTerminalEvent =
  | { event: 'output'; data: { bytesBase64: string } }
  | { event: 'exit'; data: { code: number | null; signal: string | null } }
  | { event: 'error'; data: { code: string; message: string } }

function wireEventToTerminalEvent(event: WireTerminalEvent): TerminalEvent {
  switch (event.event) {
    case 'output':
      return {
        event: 'output',
        data: { bytes: decodeBase64Bytes(event.data.bytesBase64) },
      }
    case 'exit':
      return { event: 'exit', data: event.data }
    case 'error':
      return { event: 'error', data: event.data }
  }
}

async function invokeTerminal<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    return args === undefined
      ? await invoke<T>(command)
      : await invoke<T>(command, args)
  } catch (cause) {
    throw normalizeTerminalTransportError(cause)
  }
}

export function createTauriTerminalTransport(): TerminalTransport {
  const writeTails = new Map<string, Promise<void>>()

  async function writeInput(
    conversationId: string,
    sessionId: string,
    data: string,
  ): Promise<void> {
    const normalized = toWellFormedUnicode(data)
    for (const chunk of utf8InputChunks(normalized)) {
      await invokeTerminal<void>('terminal_write', {
        conversationId,
        sessionId,
        data: chunk,
      })
    }
  }

  function enqueueWrite(
    conversationId: string,
    sessionId: string,
    data: string,
  ): Promise<void> {
    const previousTail = writeTails.get(sessionId)
    const operation = previousTail === undefined
      ? writeInput(conversationId, sessionId, data)
      : previousTail.then(() => writeInput(conversationId, sessionId, data))
    const storedTail = operation.catch(() => undefined)
    writeTails.set(sessionId, storedTail)
    void storedTail.finally(() => {
      if (writeTails.get(sessionId) === storedTail) {
        writeTails.delete(sessionId)
      }
    })
    return operation
  }

  return {
    async create(request, onEvent) {
      const onEventChannel = new Channel<WireTerminalEvent>()
      onEventChannel.onmessage = (event) => onEvent(wireEventToTerminalEvent(event))
      return invokeTerminal<TerminalDescriptor>('terminal_create', {
        request,
        onEvent: onEventChannel,
      })
    },

    write: enqueueWrite,

    resize(conversationId, sessionId, cols, rows) {
      return invokeTerminal<void>('terminal_resize', {
        conversationId,
        sessionId,
        cols,
        rows,
      })
    },

    close(conversationId, sessionId) {
      return invokeTerminal<void>('terminal_close', { conversationId, sessionId })
    },

    closeAll() {
      return invokeTerminal<void>('terminal_close_all')
    },
  }
}
