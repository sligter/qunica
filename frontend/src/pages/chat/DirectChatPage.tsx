import { useCallback, useEffect } from 'react'
import { useParams } from 'react-router-dom'
import { useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { ConversationChatView } from '@/components/chat/ConversationChatView'
import { EditableDirectChatTitle } from '@/components/direct-chats/EditableDirectChatTitle'
import { directChatQueryKey, directChatsQueryKey, replaceDirectChatInList, useDirectChat } from '@/hooks/useDirectChats'
import type { ConversationUpdatedPayload } from '@/lib/api-v2/types'
import type { DirectChatRead, GroupAgentRead } from '@/types/api'

export function DirectChatPage() {
  const { chatId } = useParams<{ chatId: string }>()
  const { t } = useTranslation('chat')
  const chat = useDirectChat(chatId)
  const qc = useQueryClient()
  useEffect(() => { if (!chat.data?.title) return; const old = document.title; document.title = `${chat.data.title} · AG Swarmer`; return () => { document.title = old } }, [chat.data?.title])
  const onUpdated = useCallback((payload: ConversationUpdatedPayload) => {
    if (!chatId || payload.conversation_id !== chatId) return
    qc.setQueryData<DirectChatRead>(directChatQueryKey(chatId), (current) => current ? { ...current, title: payload.title, title_source: payload.title_source, updated_at: payload.updated_at } : current)
    qc.setQueryData<DirectChatRead[]>(directChatsQueryKey, (current) => {
      const existing = current?.find((item) => item.id === chatId)
      return existing ? replaceDirectChatInList(current, { ...existing, title: payload.title, title_source: payload.title_source, updated_at: payload.updated_at }) : current ?? []
    })
  }, [chatId, qc])
  if (!chatId) return <div className="p-6 text-sm text-muted-foreground">{t('direct.notFound')}</div>
  if (chat.isLoading) return <div className="p-6 text-sm text-muted-foreground">{t('direct.loading')}</div>
  if (chat.error || !chat.data) return <div className="p-6 text-sm text-destructive">{t('direct.notFound')}</div>
  const item = chat.data
  const agents: GroupAgentRead[] = item.agent_id && item.agent_name ? [{ id: `${item.id}:${item.agent_id}`, group_id: item.id, agent_id: item.agent_id, display_name: item.agent_name, role: null, topology_role: null, speaking_order: 1, response_mode: 'default', workspace_mode: 'group', share_group_workspace: true, context_usage: null, status: item.agent_status ?? 'deleted', joined_at: item.created_at }] : []
  const unavailable = !item.agent_id || item.agent_status !== 'active'
  return <ConversationChatView conversationId={item.id} workspaceId={item.workspace_id} scope="direct-chats" schedulerEnabled={false} agents={agents} title={<EditableDirectChatTitle chatId={item.id} title={item.title} />} subtitle={item.agent_name ?? t('direct.agentUnavailable')} capabilities={{ showAnnouncement: false, showManage: false, showTurnTrace: false, showWorkspace: true, allowMentions: false }} disabledComposerReason={unavailable ? t('direct.agentUnavailable') : undefined} onConversationUpdated={onUpdated} />
}
