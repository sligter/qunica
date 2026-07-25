import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { ApiError, fetchFormData, fetchJson } from '@/lib/api-v2/client'
import { isDesktopRuntime, saveFileViaDialog } from '@/lib/desktop'
import { apiUrl } from '@/lib/runtime'
import { useAuthStore } from '@/stores/authStore'
import type {
  ApiErrorEnvelope,
  ConversationScope,
  ConversationWorkspaceFilePreview,
  ConversationWorkspaceFileRead,
  ConversationWorkspaceFileTextResponse,
  ConversationWorkspaceFileTextSaveRequest,
  ConversationWorkspaceFileTextSaveResponse,
  ConversationWorkspaceRoot,
} from '@/types/api'

const CONVERSATION_SCOPE_CONFIG = {
  groups: {
    routePrefix: '/groups',
    queryPrefix: 'groups',
    supportsUpload: true,
  },
  'direct-chats': {
    routePrefix: '/direct-chats',
    queryPrefix: 'direct-chats',
    supportsUpload: false,
  },
} as const satisfies Record<ConversationScope, {
  routePrefix: `/${string}`
  queryPrefix: ConversationScope
  supportsUpload: boolean
}>

export interface SaveConversationWorkspaceFileTextVariables
  extends ConversationWorkspaceFileTextSaveRequest {
  path: string
}

export interface RevocableWorkspaceFileObjectUrl {
  url: string
  revoke: () => void
}

export type ConversationWorkspaceFileMetadata = Pick<
  ConversationWorkspaceFileTextResponse,
  'path' | 'name' | 'mime_type' | 'size'
>

export class ConversationWorkspaceUploadUnsupportedError extends Error {
  readonly scope: ConversationScope

  constructor(scope: ConversationScope) {
    super(`Workspace file uploads are not supported for ${scope}`)
    this.name = 'ConversationWorkspaceUploadUnsupportedError'
    this.scope = scope
    Object.setPrototypeOf(this, ConversationWorkspaceUploadUnsupportedError.prototype)
  }
}

function scopeConfig(scope: ConversationScope) {
  return CONVERSATION_SCOPE_CONFIG[scope]
}

function requireConversationId(conversationId: string | undefined): string {
  if (!conversationId?.trim()) throw new Error('Conversation is required for workspace files')
  return conversationId
}

function requireWorkspaceFilePath(path: string): string {
  if (!path.trim()) throw new Error('Workspace file path is required')
  return path
}

function withPath(path: string): string {
  return `path=${encodeURIComponent(path)}`
}

export function conversationWorkspaceFilesApiPath(
  scope: ConversationScope,
  conversationId: string,
): string {
  const id = requireConversationId(conversationId)
  return `${scopeConfig(scope).routePrefix}/${encodeURIComponent(id)}/workspace-files`
}

function conversationWorkspaceFileEndpoint(
  scope: ConversationScope,
  conversationId: string,
  endpoint: '' | 'root' | 'preview' | 'download' | 'text' | 'text/save' | 'upload',
  path?: string,
): string {
  const suffix = endpoint ? `/${endpoint}` : ''
  const query = path === undefined ? '' : `?${withPath(path)}`
  return `${conversationWorkspaceFilesApiPath(scope, conversationId)}${suffix}${query}`
}

export function conversationWorkspaceFilesQueryKey(
  scope: ConversationScope,
  conversationId: string | undefined,
) {
  return [scopeConfig(scope).queryPrefix, conversationId, 'workspace-files'] as const
}

export function conversationWorkspaceFileListQueryKey(
  scope: ConversationScope,
  conversationId: string | undefined,
  path = '',
) {
  return [...conversationWorkspaceFilesQueryKey(scope, conversationId), 'list', path] as const
}

export function conversationWorkspaceRootQueryKey(
  scope: ConversationScope,
  conversationId: string | undefined,
) {
  return [...conversationWorkspaceFilesQueryKey(scope, conversationId), 'root'] as const
}

export function conversationWorkspaceFilePreviewQueryKey(
  scope: ConversationScope,
  conversationId: string | undefined,
  path: string | null,
) {
  return [...conversationWorkspaceFilesQueryKey(scope, conversationId), 'preview', path] as const
}

export function conversationWorkspaceFileBlobQueryKey(
  scope: ConversationScope,
  conversationId: string | undefined,
  path: string | null,
) {
  return [...conversationWorkspaceFilesQueryKey(scope, conversationId), 'blob', path] as const
}

export function conversationWorkspaceFileTextQueryKey(
  scope: ConversationScope,
  conversationId: string | undefined,
  path: string | null,
) {
  return [...conversationWorkspaceFilesQueryKey(scope, conversationId), 'text', path] as const
}

export function useConversationWorkspaceFiles(
  scope: ConversationScope,
  conversationId: string | undefined,
  path = '',
) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: conversationWorkspaceFileListQueryKey(scope, conversationId, path),
    queryFn: () =>
      fetchJson<ConversationWorkspaceFileRead[]>(
        conversationWorkspaceFileEndpoint(scope, requireConversationId(conversationId), '', path),
        { token },
      ),
    enabled: token !== null && !!conversationId,
    refetchInterval: 10_000,
  })
}

export async function getConversationWorkspaceFile(
  scope: ConversationScope,
  conversationId: string,
  path: string,
  token: string | null,
): Promise<ConversationWorkspaceFileRead | null> {
  const normalized = requireWorkspaceFilePath(path).replaceAll('\\', '/')
  const parent = normalized.includes('/') ? normalized.slice(0, normalized.lastIndexOf('/')) : ''
  const files = await fetchJson<ConversationWorkspaceFileRead[]>(
    conversationWorkspaceFileEndpoint(scope, conversationId, '', parent),
    { token },
  )
  return files.find((file) => file.path === normalized) ?? null
}

export function useConversationWorkspaceRoot(
  scope: ConversationScope,
  conversationId: string | undefined,
) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: conversationWorkspaceRootQueryKey(scope, conversationId),
    queryFn: () =>
      fetchJson<ConversationWorkspaceRoot>(
        conversationWorkspaceFileEndpoint(scope, requireConversationId(conversationId), 'root'),
        { token },
      ),
    enabled: token !== null && !!conversationId,
    staleTime: 5 * 60_000,
    retry: false,
  })
}

export function useConversationWorkspaceFilePreview(
  scope: ConversationScope,
  conversationId: string | undefined,
  path: string | null,
) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: conversationWorkspaceFilePreviewQueryKey(scope, conversationId, path),
    queryFn: () =>
      fetchJson<ConversationWorkspaceFilePreview>(
        conversationWorkspaceFileEndpoint(
          scope,
          requireConversationId(conversationId),
          'preview',
          requireWorkspaceFilePath(path ?? ''),
        ),
        { token },
      ),
    enabled: token !== null && !!conversationId && !!path,
  })
}

function isApiErrorEnvelope(value: unknown): value is ApiErrorEnvelope {
  if (typeof value !== 'object' || value === null || !('error' in value)) return false
  const error = (value as { error: unknown }).error
  return typeof error === 'object'
    && error !== null
    && 'code' in error
    && 'message' in error
    && typeof (error as { code: unknown }).code === 'string'
    && typeof (error as { message: unknown }).message === 'string'
}

async function workspaceBlobApiError(response: Response): Promise<ApiError> {
  const fallback = `HTTP ${response.status}`
  const text = await response.text()
  let parsed: unknown = null
  if (text) {
    try {
      parsed = JSON.parse(text)
    } catch {
      return new ApiError(response.status, 'http_error', text.trim() || fallback)
    }
  }
  if (isApiErrorEnvelope(parsed)) {
    return new ApiError(
      response.status,
      parsed.error.code,
      parsed.error.message,
      parsed.error.details,
    )
  }
  if (typeof parsed === 'object' && parsed !== null && 'detail' in parsed) {
    const detail = (parsed as { detail: unknown }).detail
    if (typeof detail === 'string') {
      return new ApiError(response.status, 'http_error', detail)
    }
  }
  return new ApiError(response.status, 'http_error', text.trim() || fallback)
}

export async function fetchConversationWorkspaceFileBlob(
  scope: ConversationScope,
  conversationId: string,
  path: string,
  token: string | null,
  signal?: AbortSignal,
): Promise<Blob> {
  const headers: Record<string, string> = {}
  if (token) headers.Authorization = `Bearer ${token}`
  const response = await fetch(
    apiUrl(`/api/v2${conversationWorkspaceFileEndpoint(
      scope,
      conversationId,
      'download',
      requireWorkspaceFilePath(path),
    )}`),
    { headers, signal },
  )
  if (!response.ok) throw await workspaceBlobApiError(response)
  return response.blob()
}

export function useConversationWorkspaceFileBlob(
  scope: ConversationScope,
  conversationId: string | undefined,
  path: string | null,
) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: conversationWorkspaceFileBlobQueryKey(scope, conversationId, path),
    queryFn: ({ signal }) =>
      fetchConversationWorkspaceFileBlob(
        scope,
        requireConversationId(conversationId),
        requireWorkspaceFilePath(path ?? ''),
        token,
        signal,
      ),
    enabled: token !== null && !!conversationId && !!path,
  })
}

export function useConversationWorkspaceFileText(
  scope: ConversationScope,
  conversationId: string | undefined,
  path: string | null,
) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: conversationWorkspaceFileTextQueryKey(scope, conversationId, path),
    queryFn: () => fetchConversationWorkspaceFileText(
      scope,
      requireConversationId(conversationId),
      requireWorkspaceFilePath(path ?? ''),
      token,
    ),
    enabled: token !== null && !!conversationId && !!path,
  })
}

function fetchConversationWorkspaceFileText(
  scope: ConversationScope,
  conversationId: string,
  path: string,
  token: string | null,
): Promise<ConversationWorkspaceFileTextResponse> {
  return fetchJson<ConversationWorkspaceFileTextResponse>(
    conversationWorkspaceFileEndpoint(
      scope,
      requireConversationId(conversationId),
      'text',
      requireWorkspaceFilePath(path),
    ),
    { token },
  )
}

export async function getConversationWorkspaceFileMetadata(
  scope: ConversationScope,
  conversationId: string,
  path: string,
  token: string | null,
): Promise<ConversationWorkspaceFileMetadata> {
  const file = await fetchConversationWorkspaceFileText(scope, conversationId, path, token)
  return {
    path: file.path,
    name: file.name,
    mime_type: file.mime_type,
    size: file.size,
  }
}

export function useSaveConversationWorkspaceFileText(
  scope: ConversationScope,
  conversationId: string | undefined,
) {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ path, content, version }: SaveConversationWorkspaceFileTextVariables) =>
      fetchJson<ConversationWorkspaceFileTextSaveResponse>(
        conversationWorkspaceFileEndpoint(
          scope,
          requireConversationId(conversationId),
          'text/save',
          requireWorkspaceFilePath(path),
        ),
        {
          method: 'PATCH',
          body: { content, version } satisfies ConversationWorkspaceFileTextSaveRequest,
          token,
        },
      ),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: conversationWorkspaceFilesQueryKey(scope, conversationId),
      })
    },
  })
}

export function useUploadConversationWorkspaceFile(
  scope: ConversationScope,
  conversationId: string | undefined,
) {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (file: File) => {
      const config = scopeConfig(scope)
      if (!config.supportsUpload) {
        throw new ConversationWorkspaceUploadUnsupportedError(scope)
      }
      const formData = new FormData()
      formData.append('file', file)
      return fetchFormData<ConversationWorkspaceFileRead>(
        conversationWorkspaceFileEndpoint(
          scope,
          requireConversationId(conversationId),
          'upload',
        ),
        formData,
        { token },
      )
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: conversationWorkspaceFilesQueryKey(scope, conversationId),
      })
    },
  })
}

export function createWorkspaceFileObjectUrl(blob: Blob): RevocableWorkspaceFileObjectUrl {
  const url = URL.createObjectURL(blob)
  let revoked = false
  return {
    url,
    revoke: () => {
      if (revoked) return
      revoked = true
      URL.revokeObjectURL(url)
    },
  }
}

function workspaceFileName(path: string): string {
  return path.replaceAll('\\', '/').split('/').pop() || 'download'
}

export async function downloadConversationWorkspaceFile(
  scope: ConversationScope,
  conversationId: string,
  path: string,
  token: string | null,
): Promise<void> {
  const blob = await fetchConversationWorkspaceFileBlob(scope, conversationId, path, token)
  const fileName = workspaceFileName(path)
  if (isDesktopRuntime()) {
    await saveFileViaDialog(fileName, new Uint8Array(await blob.arrayBuffer()))
    return
  }

  const objectUrl = createWorkspaceFileObjectUrl(blob)
  const link = document.createElement('a')
  try {
    link.href = objectUrl.url
    link.download = fileName
    document.body.appendChild(link)
    link.click()
  } finally {
    link.remove()
    objectUrl.revoke()
  }
}

export function useDownloadConversationWorkspaceFile(
  scope: ConversationScope,
  conversationId: string | undefined,
) {
  const token = useAuthStore((state) => state.token)
  return useMutation({
    mutationFn: (path: string) =>
      downloadConversationWorkspaceFile(
        scope,
        requireConversationId(conversationId),
        requireWorkspaceFilePath(path),
        token,
      ),
  })
}
