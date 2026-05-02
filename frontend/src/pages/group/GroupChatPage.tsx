import { useEffect } from 'react'
import { useParams } from 'react-router-dom'

import { AddAgentToGroupForm } from '@/components/chat/AddAgentToGroupForm'
import { Composer } from '@/components/chat/Composer'
import { MessageList } from '@/components/chat/MessageList'
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

  const agentNames = (groupAgents.data ?? [])
    .map((g) => `@${g.display_name}`)
    .join(' · ')
  const hint = agentNames || 'No agents in this group yet — add one below.'

  return (
    <div className="flex h-full flex-col">
      <div className="border-b border-border px-4 py-2">
        <div className="flex items-baseline justify-between gap-3">
          <div className="flex items-baseline gap-3">
            <h1 className="text-base font-semibold tracking-tight">{group.data?.name}</h1>
            {group.data?.description && (
              <span className="text-xs text-muted-foreground">
                {group.data.description}
              </span>
            )}
          </div>
        </div>
        {group.data?.announcement && (
          <p className="mt-1 text-xs text-muted-foreground">
            📣 {group.data.announcement}
          </p>
        )}
        <AddAgentToGroupForm groupId={groupId} />
      </div>

      <MessageList groupId={groupId} />

      {stream.error && (
        <div className="border-t border-border bg-red-50 px-4 py-2 text-xs text-red-700">
          Stream error: {stream.error}
        </div>
      )}

      <Composer
        isStreaming={stream.isStreaming}
        onSend={stream.send}
        onCancel={stream.cancel}
        hint={hint}
      />
    </div>
  )
}
