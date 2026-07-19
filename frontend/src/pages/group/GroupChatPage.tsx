import { useEffect } from 'react'
import { Link, useParams } from 'react-router-dom'
import { Settings } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { ConversationChatView } from '@/components/chat/ConversationChatView'
import { Button } from '@/components/ui/button'
import { useGroupAgents } from '@/hooks/useGroupAgents'
import { useGroup } from '@/hooks/useGroups'
import { normalizeLanguage } from '@/i18n'
import { formatNumber } from '@/lib/format'

export function GroupChatPage() {
  const { t, i18n } = useTranslation(['groups', 'chat'])
  const language = normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US'
  const { groupId } = useParams<{ groupId: string }>()
  const group = useGroup(groupId)
  const groupAgents = useGroupAgents(groupId)

  useEffect(() => {
    if (!group.data?.name) return
    const previousTitle = document.title
    document.title = t('documentTitle', { name: group.data.name })
    return () => {
      document.title = previousTitle
    }
  }, [group.data?.name, t])

  if (!groupId) {
    return <div className="p-6 text-sm text-muted-foreground">{t('noGroupSelected')}</div>
  }
  if (group.error) {
    return <div className="p-6 text-sm text-destructive">{t('manage.loadErrorDetail', { message: String(group.error) })}</div>
  }
  if (group.isLoading || !group.data) {
    return <div className="p-6 text-sm text-muted-foreground">{t('manage.loading')}</div>
  }

  const agents = groupAgents.data ?? []
  return (
    <ConversationChatView
      conversationId={groupId}
      scope="groups"
      schedulerEnabled={group.data.scheduler_enabled}
      agents={agents}
      title={group.data.name}
      subtitle={t('header.agent', {
        count: agents.length,
        formattedCount: formatNumber(agents.length, language),
      })}
      announcement={t('header.announcement', { text: group.data.announcement ?? '' })}
      headerActions={
        <Button variant="ghost" size="icon" asChild aria-label={t('actions.manage')}>
          <Link to={`/groups/${groupId}/manage`}>
            <Settings className="h-4 w-4" />
          </Link>
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
