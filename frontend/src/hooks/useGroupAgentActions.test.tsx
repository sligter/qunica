import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, renderHook, waitFor } from '@testing-library/react'
import type { PropsWithChildren } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type { ConversationWorkspaceRootEntry } from '@/types/api'
import { useConversationWorkspaceRoots } from './useConversationWorkspaceFiles'
import { useSetGroupAgentWorkspaceMode } from './useGroupAgentActions'
import { useUpdateAgent } from './useUpdateAgent'

vi.mock('@/lib/api-v2/client', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/lib/api-v2/client')>()
  return { ...original, fetchJson: vi.fn() }
})

const mockedFetchJson = vi.mocked(fetchJson)

const conversationRoot: ConversationWorkspaceRootEntry = {
  agent_id: null,
  display_name: null,
  workspace_mode: null,
  workspace_id: 'workspace-1',
  name: 'Shared',
  root: 'D:\\shared',
  is_primary: true,
}

const agentRoot: ConversationWorkspaceRootEntry = {
  agent_id: 'agent-1',
  display_name: 'Solo',
  workspace_mode: 'self',
  workspace_id: 'workspace-2',
  name: "Solo's",
  root: 'D:\\solo',
  is_primary: true,
}

function testClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      // Match the app: nothing refetches on its own, so a stale root listing
      // survives until something invalidates it.
      queries: { retry: false, staleTime: 30_000, refetchOnWindowFocus: false },
      mutations: { retry: false },
    },
  })
}

function wrapper(client: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>
  }
}

/** Serve the root listing from a mutable fixture; everything else echoes back. */
function serveRoots(roots: () => ConversationWorkspaceRootEntry[]) {
  mockedFetchJson.mockImplementation((path: string) =>
    Promise.resolve(path.endsWith('/workspace-roots') ? roots() : ({} as never)),
  )
}

describe('workspace root invalidation', () => {
  beforeEach(() => {
    mockedFetchJson.mockReset()
    useAuthStore.setState({ token: 'owner-token' })
  })

  afterEach(() => {
    cleanup()
    useAuthStore.setState({ token: null })
  })

  it('drops an agent root from the picker once its workspace mode is group-only', async () => {
    let roots = [conversationRoot, agentRoot]
    serveRoots(() => roots)
    const client = testClient()
    const { result } = renderHook(
      () => ({
        roots: useConversationWorkspaceRoots('groups', 'group-1'),
        setMode: useSetGroupAgentWorkspaceMode(),
      }),
      { wrapper: wrapper(client) },
    )

    await waitFor(() => expect(result.current.roots.data).toHaveLength(2))

    roots = [conversationRoot]
    await result.current.setMode.mutateAsync({
      groupId: 'group-1',
      agentId: 'agent-1',
      workspaceMode: 'group',
    })

    await waitFor(() => expect(result.current.roots.data).toEqual([conversationRoot]))
  })

  it('refreshes every conversation root listing when an agent is rebound', async () => {
    let roots = [conversationRoot, agentRoot]
    serveRoots(() => roots)
    const client = testClient()
    const { result } = renderHook(
      () => ({
        group: useConversationWorkspaceRoots('groups', 'group-1'),
        direct: useConversationWorkspaceRoots('direct-chats', 'chat-1'),
        update: useUpdateAgent('agent-1'),
      }),
      { wrapper: wrapper(client) },
    )

    await waitFor(() => expect(result.current.group.data).toHaveLength(2))
    await waitFor(() => expect(result.current.direct.data).toHaveLength(2))

    roots = [conversationRoot]
    await result.current.update.mutateAsync({ workspace_id: null })

    await waitFor(() => expect(result.current.group.data).toEqual([conversationRoot]))
    await waitFor(() => expect(result.current.direct.data).toEqual([conversationRoot]))
  })
})
