import { useEffect, useState } from 'react'
import { Link, useParams } from 'react-router-dom'
import { Files, NotebookPen, PanelRightClose, Settings, UsersRound } from 'lucide-react'

import { Composer } from '@/components/chat/Composer'
import { GroupSettingsDialog } from '@/components/chat/GroupSettingsDialog'
import { GroupWorkspaceFilesPanel } from '@/components/chat/GroupWorkspaceFilesPanel'
import { MessageList } from '@/components/chat/MessageList'
import { Button } from '@/components/ui/button'
import { useGroup } from '@/hooks/useGroups'
import { useGroupAgents } from '@/hooks/useGroupAgents'
import { useGroupMessages } from '@/hooks/useGroupMessages'
import { useSendMessageStream } from '@/hooks/useSendMessageStream'
import { useMessageStore } from '@/stores/messageStore'

export function GroupChatPage() {
  const { groupId } = useParams<{ groupId: string }>()
  const group = useGroup(groupId)
  const messagesQuery = useGroupMessages(groupId)
  const groupAgents = useGroupAgents(groupId)
  const stream = useSendMessageStream(groupId)
  const clearWarnings = useMessageStore((s) => s.clearWarnings)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [workspaceFilesOpen, setWorkspaceFilesOpen] = useState(true)

  useEffect(() => {
    if (groupId) clearWarnings(groupId)
  }, [groupId, clearWarnings])

  if (!groupId) {
    return <div className="p-6 text-sm text-muted-foreground">No group selected.</div>
  }

  if (group.error || messagesQuery.error) {
    const err = group.error ?? messagesQuery.error
    return (
      <div className="p-6 text-sm text-red-600">
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
          <h1 className="truncate text-base font-semibold tracking-tight">
            {group.data?.name}
          </h1>
          <span className="text-xs text-muted-foreground">
            {agents.length} {agents.length === 1 ? 'agent' : 'agents'}
          </span>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <Button variant="ghost" size="icon" asChild aria-label="Manage group members">
            <Link to={`/groups/${groupId}/members`}>
              <UsersRound className="h-4 w-4" />
            </Link>
          </Button>
          <Button
            variant={workspaceFilesOpen ? 'secondary' : 'ghost'}
            size="icon"
            onClick={() => setWorkspaceFilesOpen((open) => !open)}
            aria-label={workspaceFilesOpen ? 'Hide workspace files' : 'Show workspace files'}
          >
            {workspaceFilesOpen ? (
              <PanelRightClose className="h-4 w-4" />
            ) : (
              <Files className="h-4 w-4" />
            )}
          </Button>
          <Button variant="ghost" size="icon" asChild aria-label="Group notes">
            <Link to={`/groups/${groupId}/notes`}>
              <NotebookPen className="h-4 w-4" />
            </Link>
          </Button>
          <Button
            variant="ghost"
            size="icon"
            onClick={() => setSettingsOpen(true)}
            aria-label="Group settings"
          >
            <Settings className="h-4 w-4" />
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
          />

          {stream.error && (
            <div className="border-t border-border bg-red-50 px-6 py-2 text-xs text-red-700">
              Stream error: {stream.error}
            </div>
          )}

          <Composer
            isStreaming={stream.isStreaming}
            onSend={stream.send}
            onCancel={stream.cancel}
            hint={hint}
            groupAgents={agents}
          />
        </div>
        {workspaceFilesOpen && <GroupWorkspaceFilesPanel groupId={groupId} />}
      </div>

      {group.data && (
        <GroupSettingsDialog
          group={group.data}
          open={settingsOpen}
          onOpenChange={setSettingsOpen}
        />
      )}

    </div>
  )
}
