import { useEffect } from 'react'
import { useParams, useSearchParams } from 'react-router-dom'
import { ArrowLeft } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { GroupNotesPanel } from '@/components/chat/GroupNotesPanel'
import { DetailShell } from '@/components/layout/DetailShell'
import { useCloseOverlay } from '@/components/layout/overlayRouting'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { PageState } from '@/components/ui/page-state'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useGroup } from '@/hooks/useGroups'
import { GroupMembersTab } from '@/pages/group/GroupMembersTab'
import { GroupSettingsTab } from '@/pages/group/GroupSettingsTab'

/**
 * Full pane width: the members tab is a two-column master/detail, and the
 * settings tab's section rules have to end where the header's rule does — a
 * narrower cap leaves a band of dead space down the right of every setting.
 * Rows stay readable because inline controls share one right-flushed column.
 */
const MANAGE_CONTENT_WIDTH = 'max-w-none'

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
  const closeOverlay = useCloseOverlay(`/groups/${groupId ?? ''}`)

  useEffect(() => {
    const previousTitle = document.title
    document.title = t('manage.documentTitle', { name: group.data?.name ?? t('manage.title') })
    return () => {
      document.title = previousTitle
    }
  }, [group.data?.name, t])

  if (!groupId) {
    return <PageState title={t('noGroupSelected')} />
  }

  const onTabChange = (value: string) => {
    setSearchParams({ tab: value }, { replace: true })
  }

  return (
    <DetailShell
      title={t('manage.title')}
      subtitle={group.data?.name}
      leading={
        <Button variant="ghost" size="icon" aria-label={t('manage.back')} onClick={closeOverlay}>
          <ArrowLeft className="h-4 w-4" />
        </Button>
      }
      contentClassName={MANAGE_CONTENT_WIDTH}
    >
      {group.error ? (
        <PageState
          inset
          variant="error"
          className="px-0"
          title={t('manage.loadErrorDetail', { message: String(group.error) })}
        />
      ) : group.isLoading ? (
        <PageState inset variant="loading" className="px-0" title={t('manage.loading')} />
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
            <Card className="w-full p-4">
              <GroupNotesPanel groupId={groupId} />
            </Card>
          </TabsContent>
        </Tabs>
      ) : (
        <PageState inset className="px-0" title={t('manage.notFound')} />
      )}
    </DetailShell>
  )
}
