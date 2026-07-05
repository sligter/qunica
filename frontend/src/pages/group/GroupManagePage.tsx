import { Link, useParams, useSearchParams } from 'react-router-dom'
import { ArrowLeft } from 'lucide-react'

import { GroupNotesPanel } from '@/components/chat/GroupNotesPanel'
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
    <div className="flex h-full flex-col bg-background">
      <header className="flex h-14 shrink-0 items-center gap-3 border-b border-border px-6">
        <Button variant="ghost" size="icon" asChild aria-label="Back to group chat">
          <Link to={`/groups/${groupId}`}>
            <ArrowLeft className="h-4 w-4" />
          </Link>
        </Button>
        <div className="min-w-0">
          <h1 className="font-serif truncate text-base font-semibold">Manage group</h1>
          <p className="truncate text-xs text-muted-foreground">{group.data?.name}</p>
        </div>
      </header>

      {group.error ? (
        <div className="p-6 text-sm text-destructive">
          Failed to load group: {String(group.error)}
        </div>
      ) : group.isLoading ? (
        <div className="p-6 text-sm text-muted-foreground">Loading…</div>
      ) : group.data ? (
        <main className="flex-1 overflow-y-auto p-6">
          <Tabs value={tab} onValueChange={onTabChange} className="mx-auto max-w-6xl">
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
              <div className="mx-auto max-w-2xl rounded-lg border border-border bg-card p-4">
                <GroupNotesPanel groupId={groupId} />
              </div>
            </TabsContent>
          </Tabs>
        </main>
      ) : (
        <div className="p-6 text-sm text-muted-foreground">Group not found.</div>
      )}
    </div>
  )
}
