import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, renderHook } from '@testing-library/react'
import type { PropsWithChildren } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  useCreateGroupWorkspaceGitBranch,
  usePullGroupWorkspaceGit,
  useStageGroupWorkspaceGit,
  useSwitchGroupWorkspaceGitBranch,
  workspaceGitQueryKey,
} from '@/hooks/useWorkspaceGit'
import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'

vi.mock('@/lib/api-v2/client', () => ({
  fetchJson: vi.fn(),
}))

const mockedFetchJson = vi.mocked(fetchJson)

function wrapper(client: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>
  }
}

describe('workspace Git mutations', () => {
  beforeEach(() => {
    mockedFetchJson.mockReset()
    mockedFetchJson.mockResolvedValue({} as never)
    useAuthStore.setState({ token: 'owner-token' })
  })

  afterEach(() => {
    cleanup()
    useAuthStore.setState({ token: null })
  })

  it('invalidates the direct-chat file cache after pull', async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    })
    const invalidate = vi.spyOn(client, 'invalidateQueries')
    const mutation = renderHook(
      () => usePullGroupWorkspaceGit('chat-1', 'direct-chats'),
      { wrapper: wrapper(client) },
    )

    await act(async () => {
      await mutation.result.current.mutateAsync({})
    })

    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['direct-chats', 'chat-1', 'workspace-files'],
    })
  })

  it('invalidates the direct-chat file cache after switching branches', async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    })
    const invalidate = vi.spyOn(client, 'invalidateQueries')
    const mutation = renderHook(
      () => useSwitchGroupWorkspaceGitBranch('chat-1', 'direct-chats'),
      { wrapper: wrapper(client) },
    )

    await act(async () => {
      await mutation.result.current.mutateAsync({ name: 'feature', kind: 'local' })
    })

    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['direct-chats', 'chat-1', 'workspace-files'],
    })
  })

  it('stores a returned Git status immediately after staging', async () => {
    const client = new QueryClient()
    const status = { available: true, files: [] }
    mockedFetchJson.mockResolvedValueOnce(status as never)
    const mutation = renderHook(
      () => useStageGroupWorkspaceGit('group-1'),
      { wrapper: wrapper(client) },
    )

    await act(async () => {
      await mutation.result.current.mutateAsync({ paths: [] })
    })

    expect(client.getQueryData(workspaceGitQueryKey('group-1'))).toBe(status)
  })

  it('does not replace Git status with a branch-list response', async () => {
    const client = new QueryClient()
    const status = { available: true, files: [] }
    client.setQueryData(workspaceGitQueryKey('group-1'), status)
    mockedFetchJson.mockResolvedValueOnce({ branches: [] } as never)
    const mutation = renderHook(
      () => useCreateGroupWorkspaceGitBranch('group-1'),
      { wrapper: wrapper(client) },
    )

    await act(async () => {
      await mutation.result.current.mutateAsync({ name: 'feature' })
    })

    expect(client.getQueryData(workspaceGitQueryKey('group-1'))).toBe(status)
  })
})
