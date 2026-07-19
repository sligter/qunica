import { useEffect } from 'react'
import { Link, useParams, useSearchParams } from 'react-router-dom'
import { ArrowLeft } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { GroupNotesPanel } from '@/components/chat/GroupNotesPanel'
import { DetailShell } from '@/components/layout/DetailShell'
import { Button } from '@/components/ui/button'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useGroup } from '@/hooks/useGroups'
import { GroupMembersTab } from '@/pages/group/GroupMembersTab'
import { GroupSettingsTab } from '@/pages/group/GroupSettingsTab'

type ManageTab = 'settings' | 'members' | 'notes'

function parseTab(value: string | null): ManageTab {
  return value === 'members' || value === 'notes' ? value : 'settings'
}

export function GroupManagePage() {
  const { t } = useTranslation('groups')
  const { groupId } = useParams<{ groupId: string }>()
  const [searchParams, setSearchParams] = useSearchParams()
  const tab = parseTab(searchParams.get('tab'))
  const group = useGroup(groupId)

  useEffect(() => {
    const previousTitle = document.title
    document.title = t('manage.documentTitle', { name: group.data?.name ?? t('manage.title') })
    return () => {
      document.title = previousTitle
    }
  }, [group.data?.name, t])

  if (!groupId) {
    return <div className="p-6 text-sm text-muted-foreground">{t('noGroupSelected')}</div>
  }

  const onTabChange = (value: string) => {
    setSearchParams({ tab: value }, { replace: true })
  }

  return (
    <DetailShell
      title={t('manage.title')}
      subtitle={group.data?.name}
      leading={
        <Button variant="ghost" size="icon" asChild aria-label={t('manage.back')}>
          <Link to={`/groups/${groupId}`}>
            <ArrowLeft className="h-4 w-4" />
          </Link>
        </Button>
      }
      contentClassName="max-w-none"
    >
      {group.error ? (
        <div className="text-sm text-destructive">
          {t('manage.loadErrorDetail', { message: String(group.error) })}
        </div>
      ) : group.isLoading ? (
        <div className="text-sm text-muted-foreground">{t('manage.loading')}</div>
      ) : group.data ? (
        <Tabs value={tab} onValueChange={onTabChange} className="min-h-0 w-full">
          <TabsList>
            <TabsTrigger value="settings">{t('manage.settings')}</TabsTrigger>
            <TabsTrigger value="members">{t('manage.members')}</TabsTrigger>
            <TabsTrigger value="notes">{t('manage.notes')}</TabsTrigger>
          </TabsList>
          <TabsContent value="settings" className="mt-6">
            <GroupSettingsTab group={group.data} />
          </TabsContent>
          <TabsContent value="members" className="mt-6">
            <GroupMembersTab groupId={groupId} />
          </TabsContent>
          <TabsContent value="notes" className="mt-6">
            <div className="w-full rounded-lg border border-border bg-card p-4">
              <GroupNotesPanel groupId={groupId} />
            </div>
          </TabsContent>
        </Tabs>
      ) : (
        <div className="text-sm text-muted-foreground">{t('manage.notFound')}</div>
      )}
    </DetailShell>
  )
}
