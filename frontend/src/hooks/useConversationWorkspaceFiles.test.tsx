import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, renderHook, waitFor } from '@testing-library/react'
import type { PropsWithChildren } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError, fetchFormData, fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type {
  ConversationWorkspaceFilePreview,
  ConversationWorkspaceFileRead,
  ConversationWorkspaceFileTextResponse,
} from '@/types/api'
import {
  conversationWorkspaceFilesApiPath,
  conversationWorkspaceFilesQueryKey,
  createWorkspaceFileObjectUrl,
  fetchConversationWorkspaceFileBlob,
  getConversationWorkspaceFileMetadata,
  useConversationWorkspaceFilePreview,
  useConversationWorkspaceFileText,
  useConversationWorkspaceFiles,
  useSaveConversationWorkspaceFileText,
  useUploadConversationWorkspaceFile,
} from './useConversationWorkspaceFiles'
import { useWorkspaceFileActions } from './useGroupFiles'

vi.mock('@/lib/api-v2/client', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/lib/api-v2/client')>()
  return {
    ...original,
    fetchFormData: vi.fn(),
    fetchJson: vi.fn(),
  }
})

const mockedFetchJson = vi.mocked(fetchJson)
const mockedFetchFormData = vi.mocked(fetchFormData)

const fileFixture: ConversationWorkspaceFileRead = {
  path: 'docs/guide.md',
  name: 'guide.md',
  is_dir: false,
  size: 12,
  modified_at: '2026-07-25T00:00:00Z',
  abs_path: 'D:\\workspace\\docs\\guide.md',
}

const previewFixture: ConversationWorkspaceFilePreview = {
  path: fileFixture.path,
  name: fileFixture.name,
  is_text: true,
  content: 'hello',
  truncated: false,
  message: null,
  size: 5,
}

const textFixture: ConversationWorkspaceFileTextResponse = {
  path: fileFixture.path,
  name: fileFixture.name,
  mime_type: 'text/markdown',
  size: 5,
  content: 'hello',
  is_text: true,
  truncated: false,
  version: 'version-1',
  message: null,
}

function testClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })
}

function wrapper(client: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>
  }
}

describe('conversation workspace file client', () => {
  beforeEach(() => {
    mockedFetchJson.mockReset()
    mockedFetchFormData.mockReset()
    useAuthStore.setState({ token: 'owner-token' })
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
    useAuthStore.setState({ token: null })
  })

  it('uses the single scope mapping for group and direct-chat list URLs and keys', async () => {
    mockedFetchJson.mockResolvedValue([])
    const client = testClient()
    const group = renderHook(
      () => useConversationWorkspaceFiles('groups', 'group-1', 'docs'),
      { wrapper: wrapper(client) },
    )
    await waitFor(() => expect(group.result.current.isSuccess).toBe(true))

    const direct = renderHook(
      () => useConversationWorkspaceFiles('direct-chats', 'chat-1', 'nested dir'),
      { wrapper: wrapper(client) },
    )
    await waitFor(() => expect(direct.result.current.isSuccess).toBe(true))

    const hidden = renderHook(
      () => useConversationWorkspaceFiles('groups', 'group-1', 'docs', null, true),
      { wrapper: wrapper(client) },
    )
    await waitFor(() => expect(hidden.result.current.isSuccess).toBe(true))

    const searched = renderHook(
      () => useConversationWorkspaceFiles('groups', 'group-1', '', null, false, ' README guide '),
      { wrapper: wrapper(client) },
    )
    await waitFor(() => expect(searched.result.current.isSuccess).toBe(true))

    expect(conversationWorkspaceFilesApiPath('groups', 'group-1')).toBe(
      '/groups/group-1/workspace-files',
    )
    expect(conversationWorkspaceFilesApiPath('direct-chats', 'chat-1')).toBe(
      '/direct-chats/chat-1/workspace-files',
    )
    expect(conversationWorkspaceFilesQueryKey('groups', 'group-1')).toEqual([
      'groups', 'group-1', 'workspace-files',
    ])
    expect(conversationWorkspaceFilesQueryKey('direct-chats', 'chat-1')).toEqual([
      'direct-chats', 'chat-1', 'workspace-files',
    ])
    expect(mockedFetchJson).toHaveBeenCalledWith(
      '/groups/group-1/workspace-files?path=docs',
      { token: 'owner-token' },
    )
    expect(mockedFetchJson).toHaveBeenCalledWith(
      '/direct-chats/chat-1/workspace-files?path=nested%20dir',
      { token: 'owner-token' },
    )
    expect(mockedFetchJson).toHaveBeenCalledWith(
      '/groups/group-1/workspace-files?path=docs&show_hidden=true',
      { token: 'owner-token' },
    )
    expect(mockedFetchJson).toHaveBeenCalledWith(
      '/groups/group-1/workspace-files?path=&search=README%20guide',
      { token: 'owner-token' },
    )
  })

  it('builds preview and text-read URLs through the same scope mapping', async () => {
    mockedFetchJson
      .mockResolvedValueOnce(previewFixture)
      .mockResolvedValueOnce(textFixture)
    const client = testClient()
    const preview = renderHook(
      () => useConversationWorkspaceFilePreview('direct-chats', 'chat-1', 'docs/guide.md'),
      { wrapper: wrapper(client) },
    )
    await waitFor(() => expect(preview.result.current.isSuccess).toBe(true))

    const text = renderHook(
      () => useConversationWorkspaceFileText('groups', 'group-1', 'docs/guide.md'),
      { wrapper: wrapper(client) },
    )
    await waitFor(() => expect(text.result.current.isSuccess).toBe(true))

    expect(mockedFetchJson).toHaveBeenCalledWith(
      '/direct-chats/chat-1/workspace-files/preview?path=docs%2Fguide.md',
      { token: 'owner-token' },
    )
    expect(mockedFetchJson).toHaveBeenCalledWith(
      '/groups/group-1/workspace-files/text?path=docs%2Fguide.md',
      { token: 'owner-token' },
    )
  })

  it('reads server-confirmed attachment metadata through the shared text endpoint', async () => {
    mockedFetchJson.mockResolvedValueOnce(textFixture)

    await expect(getConversationWorkspaceFileMetadata(
      'direct-chats',
      'chat-1',
      'docs/guide.md',
      'owner-token',
    )).resolves.toEqual({
      path: textFixture.path,
      name: textFixture.name,
      mime_type: textFixture.mime_type,
      size: textFixture.size,
    })

    expect(mockedFetchJson).toHaveBeenCalledWith(
      '/direct-chats/chat-1/workspace-files/text?path=docs%2Fguide.md',
      { token: 'owner-token' },
    )
  })

  it('fetches authenticated blobs for both scopes without putting the token in the URL', async () => {
    const blob = new Blob(['file bytes'], { type: 'application/octet-stream' })
    const fetchMock = vi.fn().mockResolvedValue({ ok: true, blob: () => Promise.resolve(blob) })
    vi.stubGlobal('fetch', fetchMock)

    await fetchConversationWorkspaceFileBlob(
      'groups',
      'group-1',
      'docs/guide.md',
      'owner-token',
    )
    await fetchConversationWorkspaceFileBlob(
      'direct-chats',
      'chat-1',
      'images/photo.png',
      'owner-token',
    )

    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      '/api/v2/groups/group-1/workspace-files/download?path=docs%2Fguide.md',
      { headers: { Authorization: 'Bearer owner-token' }, signal: undefined, cache: 'no-store' },
    )
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      '/api/v2/direct-chats/chat-1/workspace-files/download?path=images%2Fphoto.png',
      { headers: { Authorization: 'Bearer owner-token' }, signal: undefined, cache: 'no-store' },
    )
    for (const [url] of fetchMock.mock.calls) {
      expect(String(url)).not.toContain('owner-token')
    }
  })

  it('preserves the API error envelope for authenticated blob failures', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: false,
      status: 403,
      text: () => Promise.resolve(JSON.stringify({
        error: {
          code: 'permission_denied',
          message: 'conversation belongs to another user',
          details: { conversation_id: 'chat-1' },
        },
      })),
    })
    vi.stubGlobal('fetch', fetchMock)

    let received: unknown
    try {
      await fetchConversationWorkspaceFileBlob(
        'direct-chats',
        'chat-1',
        'docs/guide.md',
        'owner-token',
      )
    } catch (error) {
      received = error
    }

    expect(received).toBeInstanceOf(ApiError)
    expect(received).toMatchObject({
      status: 403,
      code: 'permission_denied',
      message: 'conversation belongs to another user',
      details: { conversation_id: 'chat-1' },
    })
  })

  it('invalidates every current-conversation file cache after a successful save', async () => {
    mockedFetchJson.mockResolvedValueOnce({ ...textFixture, content: 'updated', version: 'version-2' })
    const client = testClient()
    const invalidate = vi.spyOn(client, 'invalidateQueries')
    const { result } = renderHook(
      () => useSaveConversationWorkspaceFileText('direct-chats', 'chat-1'),
      { wrapper: wrapper(client) },
    )

    await act(async () => {
      await result.current.mutateAsync({
        path: 'docs/guide.md',
        content: 'updated',
        version: 'version-1',
      })
    })

    expect(mockedFetchJson).toHaveBeenCalledWith(
      '/direct-chats/chat-1/workspace-files/text/save?path=docs%2Fguide.md',
      {
        method: 'PATCH',
        body: { content: 'updated', version: 'version-1' },
        token: 'owner-token',
      },
    )
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['direct-chats', 'chat-1', 'workspace-files'],
    })
  })

  it('preserves the original 409 ApiError for conflict-aware editors', async () => {
    const conflict = new ApiError(
      409,
      'conflict',
      'workspace file changed since it was read',
      { current_version: 'version-2' },
    )
    mockedFetchJson.mockRejectedValueOnce(conflict)
    const client = testClient()
    const { result } = renderHook(
      () => useSaveConversationWorkspaceFileText('groups', 'group-1'),
      { wrapper: wrapper(client) },
    )

    let received: unknown
    await act(async () => {
      try {
        await result.current.mutateAsync({
          path: 'docs/guide.md',
          content: 'local edit',
          version: 'version-1',
        })
      } catch (error) {
        received = error
      }
    })

    expect(received).toBe(conflict)
    await waitFor(() => expect(result.current.error).toBe(conflict))
  })

  it('uploads for both conversation kinds under their own namespace', async () => {
    const client = testClient()
    const upload = new File(['hello'], 'hello.txt', { type: 'text/plain' })

    mockedFetchFormData.mockResolvedValueOnce({ ...fileFixture, path: 'uploads/hello.txt' })
    const direct = renderHook(
      () => useUploadConversationWorkspaceFile('direct-chats', 'chat-1'),
      { wrapper: wrapper(client) },
    )
    await act(async () => {
      await direct.result.current.mutateAsync(upload)
    })
    expect(mockedFetchFormData.mock.calls[0]?.[0]).toBe(
      '/direct-chats/chat-1/workspace-files/upload',
    )

    mockedFetchFormData.mockResolvedValueOnce({ ...fileFixture, path: 'uploads/hello.txt' })
    const group = renderHook(
      () => useUploadConversationWorkspaceFile('groups', 'group-1'),
      { wrapper: wrapper(client) },
    )
    await act(async () => {
      await group.result.current.mutateAsync(upload)
    })
    expect(mockedFetchFormData.mock.calls[1]?.[0]).toBe('/groups/group-1/workspace-files/upload')
  })

  it('scopes reads and uploads to the selected agent root', async () => {
    const client = testClient()
    mockedFetchJson.mockResolvedValueOnce([fileFixture])
    const files = renderHook(
      () => useConversationWorkspaceFiles('groups', 'group-1', 'notes', 'agent-7'),
      { wrapper: wrapper(client) },
    )
    await waitFor(() => expect(files.result.current.isSuccess).toBe(true))
    expect(mockedFetchJson.mock.calls[0]?.[0]).toBe(
      '/groups/group-1/workspace-files?path=notes&agent_id=agent-7',
    )

    mockedFetchFormData.mockResolvedValueOnce({ ...fileFixture, path: 'uploads/hello.txt' })
    const upload = renderHook(
      () => useUploadConversationWorkspaceFile('groups', 'group-1', 'agent-7'),
      { wrapper: wrapper(client) },
    )
    await act(async () => {
      await upload.result.current.mutateAsync(
        new File(['hello'], 'hello.txt', { type: 'text/plain' }),
      )
    })
    expect(mockedFetchFormData.mock.calls[0]?.[0]).toBe(
      '/groups/group-1/workspace-files/upload?agent_id=agent-7',
    )
  })

  it('asks the server for a free name when the caller cannot choose one', async () => {
    const client = testClient()
    mockedFetchFormData.mockResolvedValueOnce({ ...fileFixture, path: 'uploads/image (1).png' })
    const { result } = renderHook(
      () => useUploadConversationWorkspaceFile('groups', 'group-1', 'agent-7', {
        uniqueName: true,
      }),
      { wrapper: wrapper(client) },
    )
    await act(async () => {
      await result.current.mutateAsync(new File(['png'], 'image.png', { type: 'image/png' }))
    })
    expect(mockedFetchFormData.mock.calls[0]?.[0]).toBe(
      '/groups/group-1/workspace-files/upload?agent_id=agent-7&unique_name=true',
    )
  })

  it('sends batch file actions to either conversation scope and refreshes file and Git state', async () => {
    mockedFetchJson.mockResolvedValue(undefined)
    const client = testClient()
    const invalidate = vi.spyOn(client, 'invalidateQueries')
    const { result } = renderHook(
      () => useWorkspaceFileActions('chat-1', 'direct-chats', 'agent-7'),
      { wrapper: wrapper(client) },
    )

    await act(async () => {
      await result.current.mutateAsync({
        action: 'move',
        paths: ['first.txt', 'folder'],
        destination: 'archive',
      })
    })

    expect(mockedFetchJson).toHaveBeenCalledWith(
      '/direct-chats/chat-1/workspace-files/actions?agent_id=agent-7',
      {
        method: 'POST',
        body: {
          action: 'move',
          paths: ['first.txt', 'folder'],
          destination: 'archive',
        },
        token: 'owner-token',
      },
    )
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['direct-chats', 'chat-1', 'workspace-files'],
    })
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['groups', 'chat-1', 'workspace-git'],
    })
  })

  it('provides an explicit, idempotent Object URL revoke lifecycle', () => {
    const createObjectURL = vi.fn(() => 'blob:workspace-file')
    const revokeObjectURL = vi.fn()
    vi.stubGlobal('URL', { createObjectURL, revokeObjectURL })

    const objectUrl = createWorkspaceFileObjectUrl(new Blob(['preview']))
    expect(objectUrl.url).toBe('blob:workspace-file')

    objectUrl.revoke()
    objectUrl.revoke()

    expect(createObjectURL).toHaveBeenCalledTimes(1)
    expect(revokeObjectURL).toHaveBeenCalledWith('blob:workspace-file')
    expect(revokeObjectURL).toHaveBeenCalledTimes(1)
  })
})
