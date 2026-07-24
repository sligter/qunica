import { renderHook } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { WorkspaceRead } from '@/types/api'
import {
  resolveTerminalConversationTarget,
  useTerminalConversationRegistration,
} from './useTerminalConversationRegistration'

const mocks = vi.hoisted(() => ({
  query: { data: undefined, isLoading: true } as {
    data: WorkspaceRead[] | undefined
    isLoading: boolean
  },
  registerConversation: vi.fn(),
  unregister: vi.fn(),
}))

vi.mock('@/hooks/useWorkspaces', () => ({
  useWorkspaces: () => mocks.query,
}))
vi.mock('@/terminal/TerminalRuntimeProvider', () => ({
  useTerminalRuntime: () => ({ registerConversation: mocks.registerConversation }),
}))

function workspace(overrides: Partial<WorkspaceRead> = {}): WorkspaceRead {
  return {
    id: 'workspace-1',
    name: 'Workspace',
    backend_type: 'local',
    local_path: 'D:/projects/example',
    sandbox_ref: null,
    config: null,
    status: 'active',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  }
}

describe('resolveTerminalConversationTarget', () => {
  it('prioritizes desktop and workspace availability before resolving paths', () => {
    expect(resolveTerminalConversationTarget(
      'chat-1', 'workspace-1', { data: undefined, isLoading: true }, false,
    )).toEqual({ conversationId: 'chat-1', availability: 'desktopRequired' })
    expect(resolveTerminalConversationTarget(
      'chat-1', null, { data: [], isLoading: false }, true,
    )).toEqual({ conversationId: 'chat-1', availability: 'workspaceRequired' })
    expect(resolveTerminalConversationTarget(
      'chat-1', 'workspace-1', { data: undefined, isLoading: true }, true,
    )).toEqual({ conversationId: 'chat-1', availability: 'loading' })
  })

  it('rejects cloud, missing, and relative local workspace paths', () => {
    expect(resolveTerminalConversationTarget(
      'chat-1', 'workspace-1',
      { data: [workspace({ backend_type: 'cloud_sandbox' })], isLoading: false },
      true,
    )).toEqual({ conversationId: 'chat-1', availability: 'localWorkspaceRequired' })
    expect(resolveTerminalConversationTarget(
      'chat-1', 'missing', { data: [workspace()], isLoading: false }, true,
    )).toEqual({ conversationId: 'chat-1', availability: 'workspaceRequired' })
    expect(resolveTerminalConversationTarget(
      'chat-1', 'workspace-1',
      { data: [workspace({ local_path: 'relative/project' })], isLoading: false },
      true,
    )).toEqual({ conversationId: 'chat-1', availability: 'pathRequired' })
  })

  it('accepts Windows, POSIX, and UNC absolute local paths', () => {
    for (const cwd of ['D:/projects/example', '/srv/example', '\\\\server\\share']) {
      expect(resolveTerminalConversationTarget(
        'chat-1', 'workspace-1',
        { data: [workspace({ local_path: cwd })], isLoading: false },
        true,
      )).toEqual({ conversationId: 'chat-1', availability: 'ready', cwd })
    }
  })
})

describe('useTerminalConversationRegistration', () => {
  beforeEach(() => {
    mocks.query = { data: undefined, isLoading: true }
    mocks.unregister.mockReset()
    mocks.registerConversation.mockReset().mockReturnValue(mocks.unregister)
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      configurable: true,
      value: {},
    })
  })

  it('updates registration as workspace data resolves and only unregisters on unmount', () => {
    const { rerender, unmount } = renderHook(
      ({ workspaceId }) => useTerminalConversationRegistration('chat-1', workspaceId),
      { initialProps: { workspaceId: 'workspace-1' as string | null } },
    )
    expect(mocks.registerConversation).toHaveBeenLastCalledWith({
      conversationId: 'chat-1', availability: 'loading',
    })

    mocks.query = { data: [workspace()], isLoading: false }
    rerender({ workspaceId: 'workspace-1' })
    expect(mocks.unregister).toHaveBeenCalledTimes(1)
    expect(mocks.registerConversation).toHaveBeenLastCalledWith({
      conversationId: 'chat-1', availability: 'ready', cwd: 'D:/projects/example',
    })

    unmount()
    expect(mocks.unregister).toHaveBeenCalledTimes(2)
  })
})
