import { useState, type ReactNode } from 'react'
import { FileText, MessageSquareText, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { WorkspacePreviewRouter } from '@/components/chat/workspace-preview/WorkspacePreviewRouter'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { cn } from '@/lib/utils'
import { useFileNavStore, type WorkspaceEditorTab } from '@/stores/fileNavStore'
import type { ConversationScope } from '@/types/api'

interface WorkspaceEditorStageProps {
  scope: ConversationScope
  conversationId: string
  children: ReactNode
}

const EMPTY_TABS: WorkspaceEditorTab[] = []

export function WorkspaceEditorStage({
  scope,
  conversationId,
  children,
}: WorkspaceEditorStageProps) {
  const { t } = useTranslation('chat')
  const stage = useFileNavStore((state) => state.editorStages[conversationId])
  const showChat = useFileNavStore((state) => state.showChat)
  const activateEditor = useFileNavStore((state) => state.activateEditor)
  const closeEditor = useFileNavStore((state) => state.closeEditor)
  const setEditorDirty = useFileNavStore((state) => state.setEditorDirty)
  const [pendingCloseId, setPendingCloseId] = useState<string | null>(null)
  const tabs = stage?.tabs ?? EMPTY_TABS
  const activeTabId = stage?.activeTabId ?? null
  const pendingClose = tabs.find((tab) => tab.id === pendingCloseId) ?? null

  if (tabs.length === 0) return <>{children}</>

  const requestClose = (tab: WorkspaceEditorTab) => {
    if (tab.dirty) setPendingCloseId(tab.id)
    else closeEditor(conversationId, tab.id)
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col bg-background">
      <div
        className="flex h-10 shrink-0 items-stretch overflow-x-auto border-b border-border bg-muted/35"
        role="tablist"
        aria-label={t('workspace.previewPanel.editorTabs')}
      >
        <button
          type="button"
          role="tab"
          aria-selected={activeTabId === null}
          className={cn(
            'flex shrink-0 items-center gap-2 border-r border-border px-4 text-xs text-muted-foreground hover:bg-background/60 hover:text-foreground',
            activeTabId === null && 'bg-background font-medium text-foreground shadow-[inset_0_2px_0_var(--color-primary)]',
          )}
          onClick={() => showChat(conversationId)}
        >
          <MessageSquareText className="h-3.5 w-3.5" aria-hidden="true" />
          {t('workspace.previewPanel.chatTab')}
        </button>

        {tabs.map((tab) => {
          const active = activeTabId === tab.id
          return (
            <div
              key={tab.id}
              className={cn(
                'group flex min-w-36 max-w-60 shrink-0 items-center border-r border-border text-muted-foreground hover:bg-background/60 hover:text-foreground',
                active && 'bg-background text-foreground shadow-[inset_0_2px_0_var(--color-primary)]',
              )}
            >
              <button
                type="button"
                role="tab"
                aria-selected={active}
                title={tab.file.path}
                className="flex min-w-0 flex-1 items-center gap-2 self-stretch pl-3 text-left text-xs"
                onClick={() => activateEditor(conversationId, tab.id)}
              >
                <FileText className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
                <span className="truncate">{tab.file.name}</span>
              </button>
              <button
                type="button"
                className="mr-1 flex h-6 w-6 shrink-0 items-center justify-center rounded-sm hover:bg-muted"
                aria-label={t('workspace.previewPanel.closeEditor', { name: tab.file.name })}
                onClick={() => requestClose(tab)}
              >
                {tab.dirty ? (
                  <span className="h-2 w-2 rounded-full bg-foreground" aria-hidden="true" />
                ) : (
                  <X className="h-3.5 w-3.5 opacity-0 group-hover:opacity-100" aria-hidden="true" />
                )}
              </button>
            </div>
          )
        })}
      </div>

      <div className={cn('min-h-0 flex-1 flex-col', activeTabId === null ? 'flex' : 'hidden')}>
        {children}
      </div>

      {tabs.map((tab) => (
        <section
          key={tab.id}
          role="tabpanel"
          aria-label={tab.file.name}
          hidden={activeTabId !== tab.id}
          className={cn(
            'min-h-0 flex-1 flex-col overflow-hidden',
            activeTabId === tab.id ? 'flex' : 'hidden',
          )}
        >
          <div className="shrink-0 border-b border-border/70 bg-background px-4 py-2 font-mono text-[11px] text-muted-foreground">
            <span className="block truncate" title={tab.file.path}>{tab.file.path}</span>
          </div>
          <div className="min-h-0 flex-1 overflow-auto p-4">
            <WorkspacePreviewRouter
              scope={scope}
              conversationId={conversationId}
              file={tab.file}
              agentId={tab.agentId}
              presentation="editor"
              onDirtyChange={(dirty) => setEditorDirty(conversationId, tab.id, dirty)}
            />
          </div>
        </section>
      ))}

      <ConfirmDialog
        open={pendingClose !== null}
        onOpenChange={(open) => {
          if (!open) setPendingCloseId(null)
        }}
        title={t('workspace.previewPanel.closeUnsavedTitle', {
          name: pendingClose?.file.name ?? '',
        })}
        description={t('workspace.previewPanel.closeUnsavedDescription')}
        confirmLabel={t('workspace.previewPanel.closeWithoutSaving')}
        destructive
        onConfirm={() => {
          if (pendingClose) closeEditor(conversationId, pendingClose.id)
          setPendingCloseId(null)
        }}
      />
    </div>
  )
}
