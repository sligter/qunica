import { useCallback, useEffect, useRef, useState } from 'react'
import { Link, useParams } from 'react-router-dom'
import { Files, PanelRightClose, Settings } from 'lucide-react'

import { Composer, type WorkspacePathInserter } from '@/components/chat/Composer'
import { GroupWorkspacePanel } from '@/components/chat/GroupWorkspacePanel'
import { MessageList } from '@/components/chat/MessageList'
import { VerticalResizeHandle } from '@/components/layout/VerticalResizeHandle'
import { Button } from '@/components/ui/button'
import { useGroup } from '@/hooks/useGroups'
import { useGroupAgents } from '@/hooks/useGroupAgents'
import { useGroupMessages } from '@/hooks/useGroupMessages'
import { usePersistentPaneWidth } from '@/hooks/usePersistentPaneWidth'
import { useSendMessageStream } from '@/hooks/useSendMessageStream'
import { useFileNavStore } from '@/stores/fileNavStore'
import { useMessageStore } from '@/stores/messageStore'

const WORKSPACE_FILES_OPEN_KEY_PREFIX = 'ag-swarmer:groups:workspace-files-open:'

type WorkspaceFilesOpenUpdater = boolean | ((current: boolean) => boolean)

function workspaceFilesOpenStorageKey(groupId: string): string {
  return `${WORKSPACE_FILES_OPEN_KEY_PREFIX}${groupId}`
}

function readWorkspaceFilesOpen(groupId: string | undefined): boolean {
  if (!groupId) return true
  const value = sessionStorage.getItem(workspaceFilesOpenStorageKey(groupId))
  return value === null ? true : value === 'true'
}

function storeWorkspaceFilesOpen(groupId: string, value: boolean): void {
  sessionStorage.setItem(workspaceFilesOpenStorageKey(groupId), String(value))
}

export function GroupChatPage() {
  const { groupId } = useParams<{ groupId: string }>()
  const group = useGroup(groupId)
  const messagesQuery = useGroupMessages(groupId)
  const groupAgents = useGroupAgents(groupId)
  const stream = useSendMessageStream(groupId)
  const clearWarnings = useMessageStore((s) => s.clearWarnings)
  const fileNavRequest = useFileNavStore((s) => s.request)
  const composerPathInserterRef = useRef<WorkspacePathInserter | null>(null)
  const [workspaceFilesOpen, setWorkspaceFilesOpen] = useState(() =>
    readWorkspaceFilesOpen(groupId),
  )
  const workspaceFilesPane = usePersistentPaneWidth({
    storageKey: 'ag-swarmer:layout:workspace-files-pane-width',
    defaultWidth: 320,
    minWidth: 260,
    maxWidth: 560,
  })

  useEffect(() => {
    setWorkspaceFilesOpen(readWorkspaceFilesOpen(groupId))
  }, [groupId])

  const setWorkspaceFilesOpenPersisted = useCallback(
    (next: WorkspaceFilesOpenUpdater) => {
      setWorkspaceFilesOpen((current) => {
        const resolved = typeof next === 'function' ? next(current) : next
        if (groupId) storeWorkspaceFilesOpen(groupId, resolved)
        return resolved
      })
    },
    [groupId],
  )

  const registerComposerPathInserter = useCallback((insert: WorkspacePathInserter | null) => {
    composerPathInserterRef.current = insert
  }, [])

  const insertWorkspacePaths = useCallback((paths: string[]) => {
    composerPathInserterRef.current?.(paths)
  }, [])

  useEffect(() => {
    if (groupId) clearWarnings(groupId)
  }, [groupId, clearWarnings])

  // A chat file link wants to show a file — make sure the panel is visible.
  useEffect(() => {
    if (fileNavRequest && fileNavRequest.groupId === groupId) {
      setWorkspaceFilesOpenPersisted(true)
    }
  }, [fileNavRequest, groupId, setWorkspaceFilesOpenPersisted])

  if (!groupId) {
    return <div className="p-6 text-sm text-muted-foreground">No group selected.</div>
  }

  if (group.error || messagesQuery.error) {
    const err = group.error ?? messagesQuery.error
    return (
      <div className="p-6 text-sm text-destructive">
        Failed to load group: {String(err)}
      </div>
    )
  }

  if (group.isLoading || messagesQuery.isLoading) {
    return <div className="p-6 text-sm text-muted-foreground">Loading…</div>
  }

  const agents = groupAgents.data ?? []
  const agentNames = agents.map((g) => `@${g.display_name}`).join(' · ')
  const hint = agentNames || 'No agents in this group yet — add one above.'

  return (
    <div className="flex h-full flex-col">
      <header className="flex h-14 shrink-0 items-center justify-between gap-4 border-b border-border bg-background px-6">
        <div className="flex min-w-0 items-baseline gap-3">
          <h1 className="font-serif truncate text-base font-semibold tracking-tight">
            {group.data?.name}
          </h1>
          <span className="text-xs text-muted-foreground">
            {agents.length} {agents.length === 1 ? 'agent' : 'agents'}
          </span>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <Button
            variant={workspaceFilesOpen ? 'secondary' : 'ghost'}
            size="icon"
            onClick={() => setWorkspaceFilesOpenPersisted((open) => !open)}
            aria-label={workspaceFilesOpen ? 'Hide workspace files' : 'Show workspace files'}
          >
            {workspaceFilesOpen ? (
              <PanelRightClose className="h-4 w-4" />
            ) : (
              <Files className="h-4 w-4" />
            )}
          </Button>
          <Button variant="ghost" size="icon" asChild aria-label="Manage group">
            <Link to={`/groups/${groupId}/manage`}>
              <Settings className="h-4 w-4" />
            </Link>
          </Button>
        </div>
      </header>

      {group.data?.announcement && (
        <div className="shrink-0 border-b border-border bg-card px-6 py-2 text-xs text-muted-foreground">
          Announcement: {group.data.announcement}
        </div>
      )}

      <div className="flex min-h-0 flex-1">
        <div className="flex min-w-0 flex-1 flex-col">
          <MessageList
            groupId={groupId}
            hasOlderMessages={messagesQuery.hasNextPage}
            isLoadingOlderMessages={messagesQuery.isFetchingNextPage}
            onLoadOlderMessages={() => void messagesQuery.fetchNextPage()}
            onSubmitHumanInput={stream.send}
          />

          {stream.error && (
            <div className="border-t border-border bg-destructive/10 px-6 py-2 text-xs text-destructive">
              Stream error: {stream.error}
            </div>
          )}

          <Composer
            groupId={groupId}
            isStreaming={stream.isStreaming}
            onSend={stream.send}
            onCancel={stream.cancel}
            hint={hint}
            groupAgents={agents}
            onRegisterWorkspacePathInserter={registerComposerPathInserter}
          />
        </div>
        {workspaceFilesOpen && (
          <>
            <VerticalResizeHandle
              label="Resize workspace files column"
              value={workspaceFilesPane.width}
              min={workspaceFilesPane.minWidth}
              max={workspaceFilesPane.maxWidth}
              increaseOnArrowRight={false}
              onResizeStart={(event) => workspaceFilesPane.startResize(event, -1)}
              onStep={workspaceFilesPane.resizeBy}
            />
            <GroupWorkspacePanel
              groupId={groupId}
              width={workspaceFilesPane.width}
              onInsertPaths={insertWorkspacePaths}
            />
          </>
        )}
      </div>

    </div>
  )
}
