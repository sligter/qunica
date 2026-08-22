import { useEffect, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'

import { ConversationWorkspacePanel } from '@/components/chat/ConversationWorkspacePanel'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { WorkspaceGitTab } from '@/components/chat/WorkspaceGitTab'
import { conversationWorkspaceFilesQueryKey } from '@/hooks/useConversationWorkspaceFiles'
import { workspaceGitQueryKey } from '@/hooks/useWorkspaceGit'
import { cn } from '@/lib/utils'
import { useFileNavStore } from '@/stores/fileNavStore'
import type { ConversationScope } from '@/types/api'

interface GroupWorkspacePanelProps {
  groupId: string | undefined
  scope?: ConversationScope
  threadId?: string
  taskBranch?: string | null
  workspaceId?: string | null
  width?: number
  className?: string
}

type WorkspaceTab = 'files' | 'git'

const WORKSPACE_TAB_KEY = 'ag-swarmer:groups:workspace-panel-tab'

function readStoredTab(): WorkspaceTab {
  return sessionStorage.getItem(WORKSPACE_TAB_KEY) === 'git' ? 'git' : 'files'
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
    const next: WorkspaceTab = value === 'git' ? 'git' : 'files'
    setTab(next)
    sessionStorage.setItem(WORKSPACE_TAB_KEY, next)
    // Refresh the data behind the tab being opened (polling stays as-is).
    if (!groupId) return
    if (next === 'git') {
      void queryClient.invalidateQueries({ queryKey: workspaceGitQueryKey(groupId, threadId) })
    } else {
      void queryClient.invalidateQueries({
        queryKey: conversationWorkspaceFilesQueryKey(scope, groupId),
      })
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
        <div className="flex h-11 shrink-0 items-center border-b border-border px-2">
          <TabsList className="grid h-8 w-full grid-cols-2 p-0.5">
            <TabsTrigger value="files" className="h-7 px-2 py-0.5 text-xs">{t('workspace.files')}</TabsTrigger>
            <TabsTrigger value="git" className="h-7 px-2 py-0.5 text-xs">{t('workspace.git')}</TabsTrigger>
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
