import { useEffect, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'

import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { WorkspaceFilesTab } from '@/components/chat/WorkspaceFilesTab'
import { WorkspaceGitTab } from '@/components/chat/WorkspaceGitTab'
import { workspaceGitQueryKey } from '@/hooks/useGroupFiles'
import { cn } from '@/lib/utils'
import { useFileNavStore } from '@/stores/fileNavStore'

interface GroupWorkspacePanelProps {
  groupId: string | undefined
  width?: number
  className?: string
  onInsertPaths?: (paths: string[]) => void
}

type WorkspaceTab = 'files' | 'git'

const WORKSPACE_TAB_KEY = 'ag-swarmer:groups:workspace-panel-tab'

function readStoredTab(): WorkspaceTab {
  return sessionStorage.getItem(WORKSPACE_TAB_KEY) === 'git' ? 'git' : 'files'
}

export function GroupWorkspacePanel({
  groupId,
  width,
  className,
  onInsertPaths,
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
      void queryClient.invalidateQueries({ queryKey: workspaceGitQueryKey(groupId) })
    } else {
      void queryClient.invalidateQueries({ queryKey: ['groups', groupId, 'workspace-files'] })
    }
  }

  // A chat link (or the Git tab) asked to open a file — show the Files tab.
  useEffect(() => {
    if (navRequest && navRequest.groupId === groupId) {
      setTab('files')
      sessionStorage.setItem(WORKSPACE_TAB_KEY, 'files')
    }
  }, [navRequest, groupId])

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
        <div className="flex h-14 shrink-0 items-center border-b border-border px-3">
          <TabsList className="grid w-full grid-cols-2">
            <TabsTrigger value="files">{t('workspace.files')}</TabsTrigger>
            <TabsTrigger value="git">{t('workspace.git')}</TabsTrigger>
          </TabsList>
        </div>
        <TabsContent value="files" className="mt-0 min-h-0 flex-1">
          <WorkspaceFilesTab groupId={groupId} onInsertPaths={onInsertPaths} />
        </TabsContent>
        <TabsContent value="git" className="mt-0 min-h-0 flex-1">
          <WorkspaceGitTab groupId={groupId} />
        </TabsContent>
      </Tabs>
    </aside>
  )
}
