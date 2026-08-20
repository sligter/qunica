import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { WorkspaceFileFallback, type WorkspaceFileMetadata } from '@/components/chat/workspace-preview/WorkspaceFileFallback'
import type { WorkspaceAgentScope } from '@/hooks/useConversationWorkspaceFiles'
import type { ConversationScope } from '@/types/api'

interface WorkspaceHtmlPreviewProps {
  scope: ConversationScope
  conversationId: string
  agentId?: WorkspaceAgentScope
  metadata: WorkspaceFileMetadata
  objectUrl: string
  onPreviewError: () => void
}

export function WorkspaceHtmlPreview({
  scope,
  conversationId,
  agentId = null,
  metadata,
  objectUrl,
  onPreviewError,
}: WorkspaceHtmlPreviewProps) {
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
        agentId={agentId}
        metadata={metadata}
        reason={t('workspace.previewPanel.htmlError')}
      />
    )
  }

  return (
    <div className="overflow-hidden rounded-lg border border-border bg-background" data-preview-kind="html">
      <iframe
        src={objectUrl}
        sandbox="allow-scripts"
        referrerPolicy="no-referrer"
        title={t('workspace.previewPanel.htmlTitle', { name: metadata.name })}
        className="h-[min(60vh,640px)] w-full bg-white"
        onError={handlePreviewError}
      />
    </div>
  )
}
