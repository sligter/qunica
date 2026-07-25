import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { WorkspaceFileFallback, type WorkspaceFileMetadata } from '@/components/chat/workspace-preview/WorkspaceFileFallback'
import type { ConversationScope } from '@/types/api'

interface WorkspacePdfPreviewProps {
  scope: ConversationScope
  conversationId: string
  metadata: WorkspaceFileMetadata
  objectUrl: string
  onPreviewError: () => void
}

export function WorkspacePdfPreview({
  scope,
  conversationId,
  metadata,
  objectUrl,
  onPreviewError,
}: WorkspacePdfPreviewProps) {
  const { t } = useTranslation('chat')
  const [failed, setFailed] = useState(false)

  useEffect(() => setFailed(false), [objectUrl])

  const handlePreviewError = () => {
    setFailed(true)
    onPreviewError()
  }

  if (failed) {
    return (
      <WorkspaceFileFallback
        scope={scope}
        conversationId={conversationId}
        metadata={metadata}
        reason={t('workspace.previewPanel.pdfError')}
      />
    )
  }

  const title = t('workspace.previewPanel.pdfTitle', { name: metadata.name })

  return (
    <object
      data={objectUrl}
      type="application/pdf"
      title={title}
      aria-label={title}
      className="h-[min(65vh,720px)] w-full rounded-lg border border-border bg-background"
      data-preview-kind="pdf"
      onError={handlePreviewError}
    >
      <WorkspaceFileFallback
        scope={scope}
        conversationId={conversationId}
        metadata={metadata}
        reason={t('workspace.previewPanel.pdfUnsupported')}
        className="m-4 rounded-lg border border-border bg-muted/25 p-4"
      />
    </object>
  )
}
