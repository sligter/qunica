import { Link, useParams } from 'react-router-dom'
import { ArrowLeft } from 'lucide-react'

import { GroupFilesPanel } from '@/components/chat/GroupFilesPanel'
import { Button } from '@/components/ui/button'
import { useGroup } from '@/hooks/useGroups'

export function GroupFilesPage() {
  const { groupId } = useParams<{ groupId: string }>()
  const group = useGroup(groupId)

  if (!groupId) {
    return <div className="p-6 text-sm text-muted-foreground">No group selected.</div>
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
          <h1 className="truncate text-base font-semibold">Group files</h1>
          <p className="truncate text-xs text-muted-foreground">{group.data?.name}</p>
        </div>
      </header>
      <main className="flex-1 overflow-y-auto p-6">
        <div className="mx-auto max-w-3xl rounded-lg border border-border bg-card p-4">
          <GroupFilesPanel groupId={groupId} embedded />
        </div>
      </main>
    </div>
  )
}
