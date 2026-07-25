import { useTranslation } from 'react-i18next'

import { WorkspaceFilesTab } from '@/components/chat/WorkspaceFilesTab'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { cn } from '@/lib/utils'
import type { ConversationScope } from '@/types/api'

export interface ConversationWorkspacePanelProps {
  scope: ConversationScope
  conversationId: string | undefined
  workspaceId: string | null
  width?: number
  className?: string
  onInsertPaths?: (paths: string[]) => void
  embedded?: boolean
}

export function ConversationWorkspacePanel({
  scope,
  conversationId,
  workspaceId,
  width,
  className,
  onInsertPaths,
  embedded = false,
}: ConversationWorkspacePanelProps) {
  const { t } = useTranslation('chat')
  const files = (
    <WorkspaceFilesTab
      scope={scope}
      conversationId={conversationId}
      workspaceId={workspaceId}
      onInsertPaths={onInsertPaths}
    />
  )

  if (embedded) return files

  return (
    <aside
      className={cn(
        'flex h-full shrink-0 flex-col border-l border-border bg-card',
        width === undefined && 'w-[280px]',
        className,
      )}
      style={width === undefined ? undefined : { width }}
    >
      <Tabs value="files" className="flex min-h-0 flex-1 flex-col">
        <div className="flex h-14 shrink-0 items-center border-b border-border px-3">
          <TabsList className="grid w-full grid-cols-1">
            <TabsTrigger value="files">{t('workspace.files')}</TabsTrigger>
          </TabsList>
        </div>
        <TabsContent value="files" className="mt-0 min-h-0 flex-1">
          {files}
        </TabsContent>
      </Tabs>
    </aside>
  )
}
