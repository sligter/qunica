import { useState } from 'react'
import { Link } from 'react-router-dom'
import { MessageSquarePlus } from 'lucide-react'

import { avatarColorClass } from '@/lib/avatarColor'
import { useGroups } from '@/hooks/useGroups'
import { GroupFormDialog } from '@/components/groups/GroupFormDialog'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'

/**
 * Chat home ("/"): a centered welcome surface shown when no group is
 * selected — serif greeting, New chat button, and a few recent chats.
 */
export function ChatHomePage() {
  const groups = useGroups()
  const [dialogOpen, setDialogOpen] = useState(false)

  const recent = (groups.data ?? []).slice(0, 5)

  return (
    <div className="flex h-full w-full flex-col items-center justify-center overflow-y-auto bg-background p-6">
      <GroupFormDialog open={dialogOpen} onOpenChange={setDialogOpen} />
      <div className="flex w-full max-w-xl flex-col items-center gap-6">
        <h1 className="text-center font-serif text-4xl font-semibold tracking-tight">
          AG Swarmer
        </h1>
        <p className="text-center text-sm text-muted-foreground">
          Start a conversation with your agents, or pick up where you left off.
        </p>
        <Button size="lg" className="gap-2 rounded-lg" onClick={() => setDialogOpen(true)}>
          <MessageSquarePlus className="h-4 w-4" />
          New chat
        </Button>

        {groups.isLoading && (
          <p className="text-xs text-muted-foreground">Loading recent chats…</p>
        )}
        {groups.error && (
          <p className="text-xs text-destructive">Failed to load recent chats.</p>
        )}
        {recent.length > 0 && (
          <div className="w-full">
            <p className="pb-2 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
              Recent chats
            </p>
            <ul className="space-y-1.5">
              {recent.map((g) => (
                <li key={g.id}>
                  <Link
                    to={`/groups/${g.id}`}
                    className="flex items-center gap-3 rounded-lg border border-border bg-card px-4 py-3 transition-colors hover:bg-card-hover"
                  >
                    <Avatar className="h-8 w-8 shrink-0">
                      <AvatarFallback className={avatarColorClass(g.id)}>
                        {g.name.slice(0, 1).toUpperCase()}
                      </AvatarFallback>
                    </Avatar>
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-sm font-medium">{g.name}</p>
                      <p className="line-clamp-1 text-xs text-muted-foreground">
                        {g.description || 'No description.'}
                      </p>
                    </div>
                  </Link>
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
    </div>
  )
}
