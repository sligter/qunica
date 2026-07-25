import { useEffect, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { LoaderCircle } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { WorkspaceFileFallback, type WorkspaceFileMetadata } from '@/components/chat/workspace-preview/WorkspaceFileFallback'
import { WorkspaceHtmlPreview } from '@/components/chat/workspace-preview/WorkspaceHtmlPreview'
import { WorkspaceImagePreview } from '@/components/chat/workspace-preview/WorkspaceImagePreview'
import { WorkspacePdfPreview } from '@/components/chat/workspace-preview/WorkspacePdfPreview'
import { WorkspaceTextEditor } from '@/components/chat/workspace-preview/WorkspaceTextEditor'
import {
  createWorkspaceFileObjectUrl,
  conversationWorkspaceFileBlobQueryKey,
  useConversationWorkspaceFileBlob,
  useConversationWorkspaceFileText,
} from '@/hooks/useConversationWorkspaceFiles'
import { workspaceErrorMessageKey } from '@/i18n/localizedError'
import type {
  ConversationScope,
  ConversationWorkspaceFileRead,
  ConversationWorkspaceFileTextResponse,
} from '@/types/api'

export type WorkspacePreviewKind = 'html' | 'image' | 'pdf' | 'text' | 'fallback'

const MAX_WORKSPACE_BINARY_PREVIEW_BYTES = 25 * 1024 * 1024

const GENERIC_MIME_TYPES = new Set(['', 'application/octet-stream', 'binary/octet-stream'])
const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp'])

function normalizedMimeType(mimeType: string): string {
  return mimeType.split(';', 1)[0]?.trim().toLowerCase() ?? ''
}

function fileExtension(path: string): string {
  const name = path.replaceAll('\\', '/').split('/').at(-1) ?? ''
  const dot = name.lastIndexOf('.')
  return dot > -1 ? name.slice(dot + 1).toLowerCase() : ''
}

function selectWorkspacePreviewKind(
  file: Pick<ConversationWorkspaceFileTextResponse, 'path' | 'mime_type' | 'is_text'>,
): WorkspacePreviewKind {
  const mime = normalizedMimeType(file.mime_type)

  if (mime === 'text/html' || mime === 'application/xhtml+xml') return 'html'
  if (mime.startsWith('image/')) return 'image'
  if (mime === 'application/pdf') return 'pdf'

  if (GENERIC_MIME_TYPES.has(mime)) {
    const extension = fileExtension(file.path)
    if (extension === 'html' || extension === 'htm') {
      return file.is_text ? 'html' : 'fallback'
    }
    if (extension === 'svg') return file.is_text ? 'image' : 'fallback'
    if (IMAGE_EXTENSIONS.has(extension)) return file.is_text ? 'fallback' : 'image'
    if (extension === 'pdf') return file.is_text ? 'fallback' : 'pdf'
  }

  return file.is_text ? 'text' : 'fallback'
}

interface WorkspaceObjectUrlState {
  blob: Blob
  url: string
}

function useWorkspaceObjectUrl(blob: Blob | undefined): string | null {
  const [state, setState] = useState<WorkspaceObjectUrlState | null>(null)

  useEffect(() => {
    if (!blob) {
      setState(null)
      return
    }
    const objectUrl = createWorkspaceFileObjectUrl(blob)
    setState({ blob, url: objectUrl.url })
    return objectUrl.revoke
  }, [blob])

  if (!state || state.blob !== blob) return null
  return state.url
}

interface WorkspacePreviewRouterProps {
  scope: ConversationScope
  conversationId: string
  file: ConversationWorkspaceFileRead
}

export function WorkspacePreviewRouter({
  scope,
  conversationId,
  file,
}: WorkspacePreviewRouterProps) {
  const { t } = useTranslation('chat')
  const queryClient = useQueryClient()
  const text = useConversationWorkspaceFileText(scope, conversationId, file.path)
  const kind = text.data ? selectWorkspacePreviewKind(text.data) : null
  const previewIdentity = `${file.path}:${kind ?? 'loading'}`
  const [failedPreview, setFailedPreview] = useState<string | null>(null)
  const previewFailed = failedPreview === previewIdentity
  const previewTooLarge = Boolean(
    text.data
    && text.data.size > MAX_WORKSPACE_BINARY_PREVIEW_BYTES
    && (kind === 'html' || kind === 'image' || kind === 'pdf'),
  )
  const needsBlob = !previewFailed
    && !previewTooLarge
    && (kind === 'html' || kind === 'image' || kind === 'pdf')
  const blob = useConversationWorkspaceFileBlob(
    scope,
    conversationId,
    needsBlob ? file.path : null,
  )
  const objectUrl = useWorkspaceObjectUrl(needsBlob ? blob.data : undefined)

  useEffect(() => setFailedPreview(null), [previewIdentity])

  useEffect(() => {
    return () => {
      queryClient.removeQueries({
        queryKey: conversationWorkspaceFileBlobQueryKey(scope, conversationId, file.path),
        exact: true,
      })
    }
  }, [conversationId, file.path, queryClient, scope])

  const fallbackMetadata: WorkspaceFileMetadata = {
    path: file.path,
    name: file.name,
    mime_type: '',
    size: file.size,
  }

  if (text.isLoading) {
    return (
      <div className="flex min-h-40 items-center justify-center gap-2 text-sm text-muted-foreground" role="status">
        <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden="true" />
        {t('workspace.previewLoading')}
      </div>
    )
  }

  if (text.error || !text.data || !kind) {
    return (
      <WorkspaceFileFallback
        scope={scope}
        conversationId={conversationId}
        metadata={fallbackMetadata}
        reason={t('workspace.filePanel.previewError', {
          message: text.error
            ? t(workspaceErrorMessageKey(text.error))
            : t('workspace.errorMessages.unexpected'),
        })}
      />
    )
  }

  const metadata: WorkspaceFileMetadata = {
    path: text.data.path,
    name: text.data.name,
    mime_type: text.data.mime_type,
    size: text.data.size,
  }

  if (kind === 'text') {
    return (
      <WorkspaceTextEditor
        scope={scope}
        conversationId={conversationId}
        file={text.data}
        onRefresh={async () => {
          const result = await text.refetch()
          if (result.error) throw result.error
          if (!result.data) throw new Error(t('workspace.previewError'))
          return result.data
        }}
      />
    )
  }

  if (kind === 'fallback') {
    return (
      <WorkspaceFileFallback
        scope={scope}
        conversationId={conversationId}
        metadata={metadata}
        reason={t('workspace.binaryPreview')}
      />
    )
  }

  if (previewTooLarge) {
    return (
      <WorkspaceFileFallback
        scope={scope}
        conversationId={conversationId}
        metadata={metadata}
        reason={t('workspace.previewPanel.previewTooLarge', {
          maxSize: MAX_WORKSPACE_BINARY_PREVIEW_BYTES / (1024 * 1024),
        })}
      />
    )
  }

  if (previewFailed) {
    const reason = kind === 'image'
      ? t('workspace.previewPanel.imageError')
      : kind === 'html'
        ? t('workspace.previewPanel.htmlError')
        : t('workspace.previewPanel.pdfError')
    return (
      <WorkspaceFileFallback
        scope={scope}
        conversationId={conversationId}
        metadata={metadata}
        reason={reason}
      />
    )
  }

  if (blob.error) {
    return (
      <WorkspaceFileFallback
        scope={scope}
        conversationId={conversationId}
        metadata={metadata}
        reason={t('workspace.previewPanel.blobError', {
          message: t(workspaceErrorMessageKey(blob.error)),
        })}
      />
    )
  }

  if (blob.isLoading || !objectUrl) {
    return (
      <div className="flex min-h-40 items-center justify-center gap-2 text-sm text-muted-foreground" role="status">
        <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden="true" />
        {t('workspace.previewLoading')}
      </div>
    )
  }

  if (kind === 'image') {
    return (
      <WorkspaceImagePreview
        scope={scope}
        conversationId={conversationId}
        metadata={metadata}
        objectUrl={objectUrl}
        onPreviewError={() => setFailedPreview(previewIdentity)}
      />
    )
  }

  if (kind === 'html') {
    return (
      <WorkspaceHtmlPreview
        scope={scope}
        conversationId={conversationId}
        metadata={metadata}
        objectUrl={objectUrl}
        onPreviewError={() => setFailedPreview(previewIdentity)}
      />
    )
  }

  return (
    <WorkspacePdfPreview
      scope={scope}
      conversationId={conversationId}
      metadata={metadata}
      objectUrl={objectUrl}
      onPreviewError={() => setFailedPreview(previewIdentity)}
    />
  )
}
