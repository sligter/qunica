import { Download, FileQuestion } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { useDownloadConversationWorkspaceFile } from '@/hooks/useConversationWorkspaceFiles'
import { normalizeLanguage } from '@/i18n'
import { workspaceErrorMessageKey } from '@/i18n/localizedError'
import { formatNumber } from '@/lib/format'
import type { ConversationScope } from '@/types/api'

export interface WorkspaceFileMetadata {
  path: string
  name: string
  mime_type: string
  size: number | null
}

function formatWorkspaceFileSize(
  size: number | null,
  language: 'en-US' | 'zh-CN',
): string {
  if (size == null) return '—'
  if (size < 1024) return `${formatNumber(size, language)} B`
  if (size < 1024 * 1024) {
    return `${formatNumber(Number((size / 1024).toFixed(1)), language)} KB`
  }
  return `${formatNumber(Number((size / (1024 * 1024)).toFixed(1)), language)} MB`
}

interface WorkspaceFileFallbackProps {
  scope: ConversationScope
  conversationId: string
  metadata: WorkspaceFileMetadata
  reason?: string
  className?: string
}

export function WorkspaceFileFallback({
  scope,
  conversationId,
  metadata,
  reason,
  className,
}: WorkspaceFileFallbackProps) {
  const { t, i18n } = useTranslation('chat')
  const language = normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US'
  const download = useDownloadConversationWorkspaceFile(scope, conversationId)

  return (
    <div
      className={className ?? 'rounded-lg border border-border bg-muted/25 p-4'}
      data-preview-kind="fallback"
    >
      <div className="flex items-start gap-3">
        <div className="rounded-md border border-border bg-background p-2 text-muted-foreground">
          <FileQuestion className="h-5 w-5" aria-hidden="true" />
        </div>
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-medium" title={metadata.name}>
            {metadata.name}
          </p>
          {reason ? <p className="mt-1 text-xs leading-5 text-muted-foreground">{reason}</p> : null}
        </div>
      </div>

      <dl className="mt-4 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-2 text-xs">
        <dt className="text-muted-foreground">{t('workspace.previewPanel.mime')}</dt>
        <dd className="min-w-0 break-all font-mono text-2xs">
          {metadata.mime_type || t('workspace.previewPanel.unknownMime')}
        </dd>
        <dt className="text-muted-foreground">{t('workspace.previewPanel.size')}</dt>
        <dd>{formatWorkspaceFileSize(metadata.size, language)}</dd>
        <dt className="text-muted-foreground">{t('workspace.previewPanel.path')}</dt>
        <dd className="min-w-0 break-all font-mono text-2xs">{metadata.path}</dd>
      </dl>

      {download.error ? (
        <p className="mt-3 text-xs text-destructive" role="alert">
          {t('workspace.previewPanel.downloadError', {
            message: t(workspaceErrorMessageKey(download.error)),
          })}
        </p>
      ) : null}

      <Button
        type="button"
        variant="outline"
        size="sm"
        className="mt-4 h-8"
        disabled={download.isPending}
        onClick={() => download.mutate(metadata.path)}
      >
        <Download className="mr-2 h-3.5 w-3.5" aria-hidden="true" />
        {download.isPending
          ? t('workspace.downloading')
          : t('workspace.previewPanel.download')}
      </Button>
    </div>
  )
}
