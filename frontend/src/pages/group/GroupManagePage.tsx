import { Link, useParams, useSearchParams } from 'react-router-dom'
import { ArrowLeft } from 'lucide-react'

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
  const { groupId } = useParams<{ groupId: string }>()
  const [searchParams, setSearchParams] = useSearchParams()
  const tab = parseTab(searchParams.get('tab'))
  const group = useGroup(groupId)

  if (!groupId) {
    return <div className="p-6 text-sm text-muted-foreground">No group selected.</div>
  }

  const onTabChange = (value: string) => {
    setSearchParams({ tab: value }, { replace: true })
  }

  return (
    <DetailShell
      title="Manage group"
      subtitle={group.data?.name}
      leading={
        <Button variant="ghost" size="icon" asChild aria-label="Back to group chat">
          <Link to={`/groups/${groupId}`}>
            <ArrowLeft className="h-4 w-4" />
          </Link>
        </Button>
      }
      contentClassName="max-w-none"
    >
      {group.error ? (
        <div className="text-sm text-destructive">
          Failed to load group: {String(group.error)}
        </div>
      ) : group.isLoading ? (
        <div className="text-sm text-muted-foreground">Loading…</div>
      ) : group.data ? (
        <Tabs value={tab} onValueChange={onTabChange} className="min-h-0 w-full">
          <TabsList>
            <TabsTrigger value="settings">Settings</TabsTrigger>
            <TabsTrigger value="members">Members</TabsTrigger>
            <TabsTrigger value="notes">Notes</TabsTrigger>
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
        <div className="text-sm text-muted-foreground">Group not found.</div>
      )}
    </DetailShell>
  )
}
