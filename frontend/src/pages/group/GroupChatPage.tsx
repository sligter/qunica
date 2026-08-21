import { useEffect, useState } from 'react'
import { useParams } from 'react-router-dom'
import { MessagesSquare, Settings } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { ConversationChatView } from '@/components/chat/ConversationChatView'
import { GroupChatHeaderActions } from '@/components/groups/GroupChatHeaderActions'
import { OverlayLink } from '@/components/layout/overlayRouting'
import { Button } from '@/components/ui/button'
import { PageState } from '@/components/ui/page-state'
import { useGroupAgents } from '@/hooks/useGroupAgents'
import { useGroup } from '@/hooks/useGroups'
import { useGroupThreads } from '@/hooks/useGroupThreads'
import { normalizeLanguage } from '@/i18n'
import { formatNumber } from '@/lib/format'

const SELECTED_THREAD_STORAGE_PREFIX = 'ag-swarmer:groups:selected-thread:'

function readSelectedThreadId(groupId: string | undefined): string | undefined {
  if (!groupId || typeof window === 'undefined') return undefined
  try {
    return window.localStorage.getItem(`${SELECTED_THREAD_STORAGE_PREFIX}${groupId}`) ?? undefined
  } catch {
    return undefined
  }
}

function persistSelectedThreadId(groupId: string, threadId: string): void {
  try {
    window.localStorage.setItem(`${SELECTED_THREAD_STORAGE_PREFIX}${groupId}`, threadId)
  } catch {
    // Task selection persistence must not block chat when storage is unavailable.
  }
}

export function GroupChatPage() {
  const { t, i18n } = useTranslation(['groups', 'chat'])
  const language = normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US'
  const { groupId } = useParams<{ groupId: string }>()
  const group = useGroup(groupId)
  const groupThreads = useGroupThreads(groupId)
  const [selection, setSelection] = useState(() => ({
    groupId,
    threadId: readSelectedThreadId(groupId),
  }))
  const selectedThreadId = selection.groupId === groupId
    ? selection.threadId
    : readSelectedThreadId(groupId)
  const threads = groupThreads.data ?? []
  const selectedThread = threads.find((thread) => thread.id === selectedThreadId)
    ?? threads.find((thread) => thread.status !== 'archived')
    ?? threads[0]
  const groupAgents = useGroupAgents(
    groupId,
    groupThreads.data ? selectedThread?.id : null,
  )

  useEffect(() => {
    if (!group.data?.name) return
    const previousTitle = document.title
    document.title = t('documentTitle', { name: group.data.name })
    return () => {
      document.title = previousTitle
    }
  }, [group.data?.name, t])

  if (!groupId) {
    return <PageState icon={MessagesSquare} title={t('noGroupSelected')} />
  }
  if (group.error) {
    return (
      <PageState
        variant="error"
        title={t('manage.loadErrorDetail', { message: String(group.error) })}
      />
    )
  }
  if (group.isLoading || !group.data) {
    return <PageState variant="loading" title={t('manage.loading')} />
  }
  if (groupThreads.error) {
    return <PageState variant="error" title={String(groupThreads.error)} />
  }
  if (groupThreads.isLoading) {
    return <PageState variant="loading" title={t('tasks.loading')} />
  }

  const agents = groupAgents.data ?? []
  const selectThread = (threadId: string) => {
    setSelection({ groupId, threadId })
    persistSelectedThreadId(groupId, threadId)
  }
  return (
    <ConversationChatView
      key={`groups:${groupId}:${selectedThread?.id ?? 'no-task'}`}
      conversationId={groupId}
      threadId={selectedThread?.id}
      workspaceId={group.data.workspace_id}
      scope="groups"
      agents={agents}
      moderatorEnabled={group.data.moderator_enabled}
      title={group.data.name}
      conversationTitle={group.data.name}
      threadTitle={selectedThread ? selectedThread.title ?? t('tasks.untitled') : undefined}
      subtitle={t('header.agent', {
        count: agents.length,
        formattedCount: formatNumber(agents.length, language),
      })}
      announcement={t('header.announcement', { text: group.data.announcement ?? '' })}
      renderHeaderContext={(disabled) => (
        <GroupChatHeaderActions
          groupId={groupId}
          threads={threads}
          selectedThread={selectedThread}
          disabled={disabled}
          onSelect={selectThread}
          onArchived={(archivedId) => {
            const next = threads.find(
              (thread) => thread.id !== archivedId && thread.status !== 'archived',
            )
            selectThread(next?.id ?? archivedId)
          }}
          onDeleted={(deletedId) => {
            const remaining = threads.filter((thread) => thread.id !== deletedId)
            const next = remaining.find((thread) => thread.status !== 'archived')
              ?? remaining[0]
            if (next) selectThread(next.id)
          }}
        />
      )}
      disabledComposerReason={
        selectedThread?.status === 'archived' ? t('tasks.archivedReadOnly') : undefined
      }
      headerActions={
        <Button variant="ghost" size="icon" asChild aria-label={t('actions.manage')}>
          <OverlayLink to={`/groups/${groupId}/manage`}>
            <Settings className="h-4 w-4" />
          </OverlayLink>
        </Button>
      }
      capabilities={{
        showAnnouncement: Boolean(group.data.announcement),
        showManage: true,
        showTurnTrace: true,
        showWorkspace: true,
        allowMentions: true,
      }}
    />
  )
}
