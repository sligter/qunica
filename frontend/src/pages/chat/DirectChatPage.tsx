import { useCallback, useEffect } from 'react'
import { useParams } from 'react-router-dom'
import { useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { ConversationChatView } from '@/components/chat/ConversationChatView'
import { EditableDirectChatTitle } from '@/components/direct-chats/EditableDirectChatTitle'
import { PageState } from '@/components/ui/page-state'
import { useAgent } from '@/hooks/useAgents'
import { directChatQueryKey, directChatsQueryKey, replaceDirectChatInList, useDirectChat } from '@/hooks/useDirectChats'
import { useProvider } from '@/hooks/useProviders'
import type { ConversationUpdatedPayload } from '@/lib/api-v2/types'
import type { DirectChatRead, GroupAgentRead } from '@/types/api'

export function DirectChatPage() {
  const { chatId } = useParams<{ chatId: string }>()
  const { t } = useTranslation('chat')
  const chat = useDirectChat(chatId)
  const qc = useQueryClient()
  // Which model answers is the agent's own configuration, not a per-message
  // choice, so the composer offers no model picker. How hard that model
  // thinks is per-question — offered only when the model accepts the setting.
  const agent = useAgent(chat.data?.agent_id ?? undefined)
  const provider = useProvider(agent.data?.llm_provider_id ?? undefined)
  const activeModel =
    (agent.data?.llm_config?.model as string | undefined) ?? provider.data?.default_model
  const supportsReasoningEffort = Boolean(
    provider.data?.models?.find((model) => model.id === activeModel)?.supports_reasoning_effort,
  )
  useEffect(() => { if (!chat.data?.title) return; const old = document.title; document.title = `${chat.data.title} · Qunica`; return () => { document.title = old } }, [chat.data?.title])
  const onUpdated = useCallback((payload: ConversationUpdatedPayload) => {
    if (!chatId || payload.conversation_id !== chatId) return
    qc.setQueryData<DirectChatRead>(directChatQueryKey(chatId), (current) => current ? { ...current, title: payload.title, title_source: payload.title_source, updated_at: payload.updated_at } : current)
    qc.setQueryData<DirectChatRead[]>(directChatsQueryKey, (current) => {
      const existing = current?.find((item) => item.id === chatId)
      return existing ? replaceDirectChatInList(current, { ...existing, title: payload.title, title_source: payload.title_source, updated_at: payload.updated_at }) : current ?? []
    })
  }, [chatId, qc])
  if (!chatId) return <PageState title={t('direct.notFound')} />
  if (chat.isLoading) return <PageState variant="loading" title={t('direct.loading')} />
  if (chat.error || !chat.data) return <PageState variant="error" title={t('direct.notFound')} />
  const item = chat.data
  const agents: GroupAgentRead[] = item.agent_id && item.agent_name ? [{ id: `${item.id}:${item.agent_id}`, group_id: item.id, agent_id: item.agent_id, display_name: item.agent_name, avatar_url: item.agent_avatar_url, role: null, topology_role: null, speaking_order: 1, response_mode: 'default', workspace_mode: 'group', share_group_workspace: true, context_usage: null, status: item.agent_status ?? 'deleted', joined_at: item.created_at }] : []
  const unavailable = !item.agent_id || item.agent_status !== 'active'
  return <ConversationChatView key={`direct-chats:${item.id}`} conversationId={item.id} workspaceId={item.workspace_id} scope="direct-chats" agents={agents} title={<EditableDirectChatTitle chatId={item.id} title={item.title} />} conversationTitle={item.title} subtitle={item.agent_name ?? t('direct.agentUnavailable')} supportsReasoningEffort={supportsReasoningEffort} capabilities={{ showAnnouncement: false, showManage: false, showTurnTrace: false, showWorkspace: true, allowMentions: false }} disabledComposerReason={unavailable ? t('direct.agentUnavailable') : undefined} onConversationUpdated={onUpdated} />
}
