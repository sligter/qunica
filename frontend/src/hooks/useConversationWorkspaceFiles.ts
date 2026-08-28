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
  ConversationWorkspaceRootEntry,
} from '@/types/api'

const CONVERSATION_SCOPE_CONFIG = {
  groups: {
    routePrefix: '/groups',
    queryPrefix: 'groups',
  },
  'direct-chats': {
    routePrefix: '/direct-chats',
    queryPrefix: 'direct-chats',
  },
} as const satisfies Record<ConversationScope, {
  routePrefix: `/${string}`
  queryPrefix: ConversationScope
}>

/**
 * Which root inside the conversation a request addresses: `null` (or omitted)
 * is the conversation's own workspace, a string is that member agent's folder.
 * Every file call carries it so a panel showing an agent's root reads, writes
 * and invalidates against that root rather than the conversation's.
 */
export type WorkspaceAgentScope = string | null | undefined

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

// Built by hand rather than with URLSearchParams: that encodes a space as `+`,
// and these paths have always been percent-encoded.
function workspaceQuery(
  path: string | undefined,
  agentId: WorkspaceAgentScope,
  showHidden = false,
  uniqueName = false,
  search = '',
): string {
  const parts: string[] = []
  if (path !== undefined) parts.push(`path=${encodeURIComponent(path)}`)
  if (agentId) parts.push(`agent_id=${encodeURIComponent(agentId)}`)
  if (showHidden) parts.push('show_hidden=true')
  if (uniqueName) parts.push('unique_name=true')
  if (search) parts.push(`search=${encodeURIComponent(search)}`)
  return parts.length > 0 ? `?${parts.join('&')}` : ''
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
  agentId?: WorkspaceAgentScope,
  showHidden?: boolean,
  uniqueName?: boolean,
  search?: string,
): string {
  const suffix = endpoint ? `/${endpoint}` : ''
  return `${conversationWorkspaceFilesApiPath(scope, conversationId)}${suffix}${workspaceQuery(path, agentId, showHidden, uniqueName, search)}`
}

export function conversationWorkspaceRootsApiPath(
  scope: ConversationScope,
  conversationId: string,
): string {
  const id = requireConversationId(conversationId)
  return `${scopeConfig(scope).routePrefix}/${encodeURIComponent(id)}/workspace-roots`
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
  agentId: WorkspaceAgentScope = null,
  showHidden = false,
  search = '',
) {
  return [
    ...conversationWorkspaceFilesQueryKey(scope, conversationId),
    'list',
    agentId ?? null,
    path,
    showHidden,
    search,
  ] as const
}

export function conversationWorkspaceRootQueryKey(
  scope: ConversationScope,
  conversationId: string | undefined,
  agentId: WorkspaceAgentScope = null,
) {
  return [
    ...conversationWorkspaceFilesQueryKey(scope, conversationId),
    'root',
    agentId ?? null,
  ] as const
}

export function conversationWorkspaceRootsQueryKey(
  scope: ConversationScope,
  conversationId: string | undefined,
) {
  return [scopeConfig(scope).queryPrefix, conversationId, 'workspace-roots'] as const
}

/**
 * Matches the root listing of every conversation, in either scope. An agent's
 * own workspace can surface in any conversation it belongs to, so a change to
 * the agent itself has to invalidate all of them, not one known id.
 */
export function isConversationWorkspaceRootsQueryKey(queryKey: readonly unknown[]): boolean {
  return queryKey[2] === 'workspace-roots'
}

export function conversationWorkspaceFilePreviewQueryKey(
  scope: ConversationScope,
  conversationId: string | undefined,
  path: string | null,
  agentId: WorkspaceAgentScope = null,
) {
  return [
    ...conversationWorkspaceFilesQueryKey(scope, conversationId),
    'preview',
    agentId ?? null,
    path,
  ] as const
}

export function conversationWorkspaceFileBlobQueryKey(
  scope: ConversationScope,
  conversationId: string | undefined,
  path: string | null,
  agentId: WorkspaceAgentScope = null,
) {
  return [
    ...conversationWorkspaceFilesQueryKey(scope, conversationId),
    'blob',
    agentId ?? null,
    path,
  ] as const
}

export function conversationWorkspaceFileTextQueryKey(
  scope: ConversationScope,
  conversationId: string | undefined,
  path: string | null,
  agentId: WorkspaceAgentScope = null,
) {
  return [
    ...conversationWorkspaceFilesQueryKey(scope, conversationId),
    'text',
    agentId ?? null,
    path,
  ] as const
}

export function useConversationWorkspaceFiles(
  scope: ConversationScope,
  conversationId: string | undefined,
  path = '',
  agentId: WorkspaceAgentScope = null,
  showHidden = false,
  search = '',
) {
  const token = useAuthStore((state) => state.token)
  const normalizedSearch = search.trim()
  return useQuery({
    queryKey: conversationWorkspaceFileListQueryKey(
      scope,
      conversationId,
      path,
      agentId,
      showHidden,
      normalizedSearch,
    ),
    queryFn: () =>
      fetchJson<ConversationWorkspaceFileRead[]>(
        conversationWorkspaceFileEndpoint(
          scope,
          requireConversationId(conversationId),
          '',
          path,
          agentId,
          showHidden,
          undefined,
          normalizedSearch,
        ),
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
  agentId: WorkspaceAgentScope = null,
) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: conversationWorkspaceRootQueryKey(scope, conversationId, agentId),
    queryFn: () =>
      fetchJson<ConversationWorkspaceRoot>(
        conversationWorkspaceFileEndpoint(
          scope,
          requireConversationId(conversationId),
          'root',
          undefined,
          agentId,
        ),
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
  agentId: WorkspaceAgentScope = null,
) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: conversationWorkspaceFilePreviewQueryKey(scope, conversationId, path, agentId),
    queryFn: () =>
      fetchJson<ConversationWorkspaceFilePreview>(
        conversationWorkspaceFileEndpoint(
          scope,
          requireConversationId(conversationId),
          'preview',
          requireWorkspaceFilePath(path ?? ''),
          agentId,
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
  agentId: WorkspaceAgentScope = null,
): Promise<Blob> {
  const headers: Record<string, string> = {}
  if (token) headers.Authorization = `Bearer ${token}`
  const response = await fetch(
    apiUrl(`/api/v2${conversationWorkspaceFileEndpoint(
      scope,
      conversationId,
      'download',
      requireWorkspaceFilePath(path),
      agentId,
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
  agentId: WorkspaceAgentScope = null,
) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: conversationWorkspaceFileBlobQueryKey(scope, conversationId, path, agentId),
    queryFn: ({ signal }) =>
      fetchConversationWorkspaceFileBlob(
        scope,
        requireConversationId(conversationId),
        requireWorkspaceFilePath(path ?? ''),
        token,
        signal,
        agentId,
      ),
    enabled: token !== null && !!conversationId && !!path,
    gcTime: 0,
  })
}

export function useConversationWorkspaceFileText(
  scope: ConversationScope,
  conversationId: string | undefined,
  path: string | null,
  agentId: WorkspaceAgentScope = null,
) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: conversationWorkspaceFileTextQueryKey(scope, conversationId, path, agentId),
    queryFn: () => fetchConversationWorkspaceFileText(
      scope,
      requireConversationId(conversationId),
      requireWorkspaceFilePath(path ?? ''),
      token,
      agentId,
    ),
    enabled: token !== null && !!conversationId && !!path,
  })
}

function fetchConversationWorkspaceFileText(
  scope: ConversationScope,
  conversationId: string,
  path: string,
  token: string | null,
  agentId: WorkspaceAgentScope = null,
): Promise<ConversationWorkspaceFileTextResponse> {
  return fetchJson<ConversationWorkspaceFileTextResponse>(
    conversationWorkspaceFileEndpoint(
      scope,
      requireConversationId(conversationId),
      'text',
      requireWorkspaceFilePath(path),
      agentId,
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
  agentId: WorkspaceAgentScope = null,
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
          agentId,
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

export interface UploadConversationWorkspaceFileOptions {
  /**
   * Let the server pick a free name (`image (1).png`) when the upload folder
   * already holds that filename. Callers whose names are generated rather than
   * chosen — pasted screenshots all arrive as `image.png` — need this, or the
   * second one fails the conflict check with nothing the user can do about it.
   */
  uniqueName?: boolean
}

export function useUploadConversationWorkspaceFile(
  scope: ConversationScope,
  conversationId: string | undefined,
  agentId: WorkspaceAgentScope = null,
  { uniqueName = false }: UploadConversationWorkspaceFileOptions = {},
) {
  const token = useAuthStore((state) => state.token)
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (file: File) => {
      const formData = new FormData()
      formData.append('file', file)
      return fetchFormData<ConversationWorkspaceFileRead>(
        conversationWorkspaceFileEndpoint(
          scope,
          requireConversationId(conversationId),
          'upload',
          undefined,
          agentId,
          undefined,
          uniqueName,
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
  agentId: WorkspaceAgentScope = null,
): Promise<void> {
  const blob = await fetchConversationWorkspaceFileBlob(
    scope,
    conversationId,
    path,
    token,
    undefined,
    agentId,
  )
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
  agentId: WorkspaceAgentScope = null,
) {
  const token = useAuthStore((state) => state.token)
  return useMutation({
    mutationFn: (path: string) =>
      downloadConversationWorkspaceFile(
        scope,
        requireConversationId(conversationId),
        requireWorkspaceFilePath(path),
        token,
        agentId,
      ),
  })
}

/**
 * The roots a viewer may browse for this conversation: its own workspace plus
 * any member agent folder that is actually reachable during that agent's turns.
 */
export function useConversationWorkspaceRoots(
  scope: ConversationScope,
  conversationId: string | undefined,
) {
  const token = useAuthStore((state) => state.token)
  return useQuery({
    queryKey: conversationWorkspaceRootsQueryKey(scope, conversationId),
    queryFn: () =>
      fetchJson<ConversationWorkspaceRootEntry[]>(
        conversationWorkspaceRootsApiPath(scope, requireConversationId(conversationId)),
        { token },
      ),
    enabled: token !== null && !!conversationId,
    staleTime: 60_000,
  })
}
