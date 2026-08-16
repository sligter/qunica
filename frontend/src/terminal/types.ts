import type { ShellPreference } from '@/types/api'

export type TerminalConversationTarget =
  | { conversationId: string; availability: 'ready'; cwd: string }
  | { conversationId: string; availability: 'loading' }
  | { conversationId: string; availability: 'desktopRequired' }
  | { conversationId: string; availability: 'workspaceRequired' }
  | { conversationId: string; availability: 'localWorkspaceRequired' }
  | { conversationId: string; availability: 'pathRequired' }

export interface CreateTerminalRequest {
  conversationId: string
  cwd: string
  cols: number
  rows: number
  /** Which interpreter to start; omitted means the host default. */
  shell?: ShellPreference
}

export interface TerminalDescriptor {
  sessionId: string
  shellName: string
  cwd: string
}

export type TerminalEvent =
  | { event: 'output'; data: { bytes: Uint8Array } }
  | { event: 'exit'; data: { code: number | null; signal: string | null } }
  | { event: 'error'; data: { code: string; message: string } }
