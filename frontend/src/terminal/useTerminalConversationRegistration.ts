import { useEffect, useMemo } from 'react'

import { useWorkspaces } from '@/hooks/useWorkspaces'
import { looksAbsolute } from '@/lib/folderPicker'
import { isDesktopRuntime } from '@/lib/runtime'
import {
  useOptionalTerminalRuntime,
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
  cwdOverride?: string | null,
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

  const cwd = cwdOverride?.trim() || workspace.local_path?.trim() || ''
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
  cwdOverride?: string | null,
): TerminalConversationTarget {
  const workspaces = useWorkspaces()
  const { registerConversation } = useTerminalRuntime()
  const target = useMemo(
    () => resolveTerminalConversationTarget(
      conversationId,
      workspaceId,
      { data: workspaces.data, isLoading: workspaces.isLoading },
      undefined,
      cwdOverride,
    ),
    [conversationId, cwdOverride, workspaceId, workspaces.data, workspaces.isLoading],
  )

  useEffect(
    () => registerConversation(target),
    [registerConversation, target],
  )

  return target
}

/**
 * Registers the chat only when a terminal runtime is mounted. Compact surfaces
 * such as the Assistant window never host a terminal, so they skip this.
 */
export function useOptionalTerminalConversationRegistration(
  conversationId: string | undefined,
  workspaceId: string | null,
  cwdOverride?: string | null,
): void {
  const workspaces = useWorkspaces()
  const runtime = useOptionalTerminalRuntime()
  const registerConversation = runtime?.registerConversation
  const target = useMemo(
    () => conversationId
      ? resolveTerminalConversationTarget(
          conversationId,
          workspaceId,
          { data: workspaces.data, isLoading: workspaces.isLoading },
          undefined,
          cwdOverride,
        )
      : null,
    [conversationId, cwdOverride, workspaceId, workspaces.data, workspaces.isLoading],
  )

  useEffect(() => {
    if (!registerConversation || !target) return
    return registerConversation(target)
  }, [registerConversation, target])
}
