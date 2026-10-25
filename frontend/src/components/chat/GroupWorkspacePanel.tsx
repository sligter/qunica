import { lazy, Suspense, useEffect, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { CalendarClock, FolderOpen, GitBranch, Monitor } from 'lucide-react'

import { ConversationWorkspacePanel } from '@/components/chat/ConversationWorkspacePanel'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { WorkspaceGitTab } from '@/components/chat/WorkspaceGitTab'
import { conversationWorkspaceFilesQueryKey } from '@/hooks/useConversationWorkspaceFiles'
import { workspaceGitQueryKey } from '@/hooks/useWorkspaceGit'
import { cn } from '@/lib/utils'
import { useFileNavStore } from '@/stores/fileNavStore'
import type { ConversationScope } from '@/types/api'

// Note: inspectors mount only while selected. Closing a desktop also closes
// its connection and releases human control instead of keeping a hidden driver.
const ConversationComputerTab = lazy(() => import('./ConversationComputerTab').then(module => ({ default: module.ConversationComputerTab })))
const ConversationTriggersTab = lazy(() => import('./ConversationTriggersTab').then(module => ({ default: module.ConversationTriggersTab })))

interface GroupWorkspacePanelProps {
  groupId: string | undefined
  scope?: ConversationScope
  threadId?: string
  taskBranch?: string | null
  workspaceId?: string | null
  width?: number
  className?: string
}

type WorkspaceTab = 'files' | 'git' | 'computer' | 'triggers'

const WORKSPACE_TAB_KEY = 'qunica:groups:workspace-panel-tab'

function readStoredTab(): WorkspaceTab {
  const value = sessionStorage.getItem(WORKSPACE_TAB_KEY)
  return value === 'git' || value === 'computer' || value === 'triggers' ? value : 'files'
}

export function GroupWorkspacePanel({
  groupId,
  scope = 'groups',
  threadId,
  taskBranch,
  workspaceId = null,
  width,
  className,
}: GroupWorkspacePanelProps) {
  const { t } = useTranslation('chat')
  const [tab, setTab] = useState<WorkspaceTab>(() => readStoredTab())
  const navRequest = useFileNavStore((s) => s.request)
  const queryClient = useQueryClient()

  const changeTab = (value: string) => {
    const next: WorkspaceTab = value === 'git' || value === 'computer' || value === 'triggers' ? value : 'files'
    setTab(next)
    sessionStorage.setItem(WORKSPACE_TAB_KEY, next)
    // Refresh the data behind the tab being opened (polling stays as-is).
    if (!groupId) return
    if (next === 'git') {
      void queryClient.invalidateQueries({ queryKey: workspaceGitQueryKey(groupId, threadId) })
    } else if (next === 'files') {
      void queryClient.invalidateQueries({
        queryKey: conversationWorkspaceFilesQueryKey(scope, groupId),
      })
    } else {
      void queryClient.invalidateQueries({ queryKey: next === 'computer' ? ['computers'] : ['automations', groupId] })
    }
  }

  // A chat link (or the Git tab) asked to open a file — show the Files tab.
  useEffect(() => {
    if (navRequest && navRequest.groupId === groupId) {
      setTab('files')
      sessionStorage.setItem(WORKSPACE_TAB_KEY, 'files')
    }
  }, [navRequest, groupId, scope])

  return (
    <aside
      className={cn(
        'flex h-full shrink-0 flex-col border-l border-border bg-card',
        width === undefined && 'w-[280px]',
        className,
      )}
      style={width === undefined ? undefined : { width }}
    >
      <Tabs value={tab} onValueChange={changeTab} className="flex min-h-0 flex-1 flex-col">
        <div className="workspace-panel-tabs flex h-11 shrink-0 items-center border-b border-border px-2">
          <TabsList className="grid h-8 w-full grid-cols-4 p-0.5">
            <TabsTrigger value="files" aria-label={t('workspace.files')} title={t('workspace.files')} className="h-7 min-w-0 gap-1 px-1 py-0.5">
              <FolderOpen className="h-4 w-4" aria-hidden="true" />
              <span className="workspace-tab-label hidden">{t('workspace.files')}</span>
            </TabsTrigger>
            <TabsTrigger value="git" aria-label={t('workspace.git')} title={t('workspace.git')} className="h-7 min-w-0 gap-1 px-1 py-0.5">
              <GitBranch className="h-4 w-4" aria-hidden="true" />
              <span className="workspace-tab-label hidden">{t('workspace.git')}</span>
            </TabsTrigger>
            <TabsTrigger value="computer" aria-label={t('workspace.computer')} title={t('workspace.computer')} className="h-7 min-w-0 gap-1 px-1 py-0.5">
              <Monitor className="h-4 w-4" aria-hidden="true" />
              <span className="workspace-tab-label hidden">{t('workspace.computer')}</span>
            </TabsTrigger>
            <TabsTrigger value="triggers" aria-label={t('workspace.triggers')} title={t('workspace.triggers')} className="h-7 min-w-0 gap-1 px-1 py-0.5">
              <CalendarClock className="h-4 w-4" aria-hidden="true" />
              <span className="workspace-tab-label hidden">{t('workspace.triggers')}</span>
            </TabsTrigger>
          </TabsList>
        </div>
        <TabsContent value="files" className="mt-0 min-h-0 flex-1">
          <ConversationWorkspacePanel
            embedded
            scope={scope}
            conversationId={groupId}
            workspaceId={workspaceId}
          />
        </TabsContent>
        <TabsContent value="computer" className="mt-0 min-h-0 flex-1">
          <Suspense fallback={<p role="status" className="p-4 text-xs text-muted-foreground">{t('workspace.panelLoading')}</p>}>
            <ConversationComputerTab key={`${scope}:${groupId}`} scope={scope} conversationId={groupId} />
          </Suspense>
        </TabsContent>
        <TabsContent value="triggers" className="mt-0 min-h-0 flex-1">
          <Suspense fallback={<p role="status" className="p-4 text-xs text-muted-foreground">{t('workspace.panelLoading')}</p>}>
            <ConversationTriggersTab key={`${scope}:${groupId}`} scope={scope} conversationId={groupId} />
          </Suspense>
        </TabsContent>
        <TabsContent value="git" className="mt-0 min-h-0 flex-1">
          <WorkspaceGitTab
            groupId={groupId}
            scope={scope}
            threadId={threadId}
            taskBranch={taskBranch}
          />
        </TabsContent>
      </Tabs>
    </aside>
  )
}
