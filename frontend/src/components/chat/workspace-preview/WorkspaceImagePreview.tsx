import { useEffect, useState } from 'react'
import { Maximize2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { WorkspaceFileFallback, type WorkspaceFileMetadata } from '@/components/chat/workspace-preview/WorkspaceFileFallback'
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'
import type { WorkspaceAgentScope } from '@/hooks/useConversationWorkspaceFiles'
import type { ConversationScope } from '@/types/api'

interface WorkspaceImagePreviewProps {
  scope: ConversationScope
  conversationId: string
  agentId?: WorkspaceAgentScope
  metadata: WorkspaceFileMetadata
  objectUrl: string
  onPreviewError: () => void
}

export function WorkspaceImagePreview({
  scope,
  conversationId,
  agentId = null,
  metadata,
  objectUrl,
  onPreviewError,
}: WorkspaceImagePreviewProps) {
  const { t } = useTranslation('chat')
  const [failed, setFailed] = useState(false)
  const [lightboxOpen, setLightboxOpen] = useState(false)

  useEffect(() => {
    setFailed(false)
    setLightboxOpen(false)
  }, [objectUrl])

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
        reason={t('workspace.previewPanel.imageError')}
      />
    )
  }

  return (
    <div className="h-full" data-preview-kind="image">
      <button
        type="button"
        className="group relative grid h-full min-h-80 w-full place-items-center overflow-hidden bg-muted/20 p-4 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
        onClick={() => setLightboxOpen(true)}
        aria-label={t('workspace.previewPanel.openImage', { name: metadata.name })}
      >
        <img
          src={objectUrl}
          alt={metadata.name}
          className="max-h-full max-w-full object-contain drop-shadow-sm"
          onError={handlePreviewError}
        />
        <span className="absolute bottom-4 right-4 rounded-md border border-white/15 bg-black/65 p-2 text-white opacity-0 shadow-sm transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100">
          <Maximize2 className="h-3.5 w-3.5" aria-hidden="true" />
        </span>
      </button>

      <Dialog open={lightboxOpen} onOpenChange={setLightboxOpen}>
        <DialogContent
          closeLabel={t('workspace.previewPanel.closeImage')}
          aria-describedby={undefined}
          className="w-[min(96vw,1120px)] max-w-none border-0 bg-transparent p-2 shadow-none"
        >
          <DialogTitle className="sr-only">
            {t('workspace.previewPanel.imageTitle', { name: metadata.name })}
          </DialogTitle>
          <img
            src={objectUrl}
            alt={metadata.name}
            className="max-h-[88vh] w-full object-contain"
            onError={handlePreviewError}
          />
        </DialogContent>
      </Dialog>
    </div>
  )
}
