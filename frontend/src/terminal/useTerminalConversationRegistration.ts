import { useEffect, useMemo } from 'react'

import { useWorkspaces } from '@/hooks/useWorkspaces'
import { looksAbsolute } from '@/lib/folderPicker'
import { isDesktopRuntime } from '@/lib/runtime'
import {
  useTerminalRuntime,
} from '@/terminal/TerminalRuntimeProvider'
import type { TerminalConversationTarget } from '@/terminal/types'
import type { WorkspaceRead } from '@/types/api'

interface WorkspaceQueryState {
  data: WorkspaceRead[] | undefined
  isLoading: boolean
}

export function resolveTerminalConversationTarget(
  conversationId: string,
  workspaceId: string | null,
  workspaces: WorkspaceQueryState,
  desktop = isDesktopRuntime(),
): TerminalConversationTarget {
  if (!desktop) {
    return { conversationId, availability: 'desktopRequired' }
  }
  if (workspaceId === null) {
    return { conversationId, availability: 'workspaceRequired' }
  }
  if (workspaces.isLoading || workspaces.data === undefined) {
    return { conversationId, availability: 'loading' }
  }

  const workspace = workspaces.data.find((candidate) => candidate.id === workspaceId)
  if (workspace === undefined) {
    return { conversationId, availability: 'workspaceRequired' }
  }
  if (workspace.backend_type !== 'local') {
    return { conversationId, availability: 'localWorkspaceRequired' }
  }

  const cwd = workspace.local_path?.trim() ?? ''
  if (cwd === '' || !looksAbsolute(cwd)) {
    return { conversationId, availability: 'pathRequired' }
  }
  return { conversationId, availability: 'ready', cwd }
}

/**
 * Registers the mounted chat as the active terminal target. The returned
 * cleanup only unregisters the page; it intentionally leaves PTYs running.
 */
export function useTerminalConversationRegistration(
  conversationId: string,
  workspaceId: string | null,
): TerminalConversationTarget {
  const workspaces = useWorkspaces()
  const { registerConversation } = useTerminalRuntime()
  const target = useMemo(
    () => resolveTerminalConversationTarget(
      conversationId,
      workspaceId,
      { data: workspaces.data, isLoading: workspaces.isLoading },
    ),
    [conversationId, workspaceId, workspaces.data, workspaces.isLoading],
  )

  useEffect(
    () => registerConversation(target),
    [registerConversation, target],
  )

  return target
}
