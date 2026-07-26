import { lazy, Suspense, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'
import { MessageSquarePlus } from 'lucide-react'

import { avatarColorClass } from '@/lib/avatarColor'
import { useGroups } from '@/hooks/useGroups'
import { useDirectChats } from '@/hooks/useDirectChats'
import { DirectChatPickerDialog } from '@/components/direct-chats/DirectChatPickerDialog'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'

// Shares the chunk the sidebar's New group button loads; neither pays for it
// until someone actually opens the create-group form.
const GroupFormDialog = lazy(() =>
  import('@/components/groups/GroupFormDialog').then((m) => ({ default: m.GroupFormDialog })),
)

/**
 * Chat home ("/"): a centered welcome surface shown when no group is
 * selected — serif greeting, New group button, and a few recent groups.
 */
export function ChatHomePage() {
  const { t } = useTranslation(['groups', 'navigation'])
  const groups = useGroups()
  const directChats = useDirectChats()
  const [dialogOpen, setDialogOpen] = useState(false)
  const [directDialogOpen, setDirectDialogOpen] = useState(false)

  const recent = [
    ...(groups.data ?? []).map((group) => ({
      id: group.id,
      kind: 'group' as const,
      title: group.name,
      subtitle: group.description || t('groups:noDescription'),
      updatedAt: group.updated_at ?? group.created_at,
      to: `/groups/${group.id}`,
    })),
    ...(directChats.data ?? []).map((chat) => ({
      id: chat.id,
      kind: 'direct' as const,
      title: chat.title,
      subtitle: chat.agent_name ?? t('chat:direct.agentUnavailable'),
      updatedAt: chat.updated_at,
      to: `/chats/${chat.id}`,
    })),
  ].sort((left, right) => right.updatedAt.localeCompare(left.updatedAt)).slice(0, 5)
  const pageTitle = t('groups:pageTitle')

  useEffect(() => {
    document.title = pageTitle
  }, [pageTitle])

  return (
    <div className="flex h-full w-full flex-col items-center justify-center overflow-y-auto bg-background p-6">
      {dialogOpen ? (
        <Suspense fallback={null}>
          <GroupFormDialog open onOpenChange={setDialogOpen} />
        </Suspense>
      ) : null}
      <DirectChatPickerDialog open={directDialogOpen} onOpenChange={setDirectDialogOpen} />
      <div className="flex w-full max-w-xl flex-col items-center gap-6">
        <h1 className="text-center font-serif text-4xl font-semibold tracking-tight">
          AG Swarmer
        </h1>
        <p className="text-center text-sm text-muted-foreground">
          {t('groups:homeSubtitle')}
        </p>
        <div className="flex flex-wrap justify-center gap-2">
          <Button size="lg" className="gap-2 rounded-lg" onClick={() => setDirectDialogOpen(true)}>
            <MessageSquarePlus className="h-4 w-4" />
            {t('navigation:newDirectChat')}
          </Button>
          <Button size="lg" variant="outline" className="gap-2 rounded-lg" onClick={() => setDialogOpen(true)}>
            <MessageSquarePlus className="h-4 w-4" />
            {t('navigation:newGroup')}
          </Button>
        </div>

        {(groups.isLoading || directChats.isLoading) && (
          <p className="text-xs text-muted-foreground">{t('groups:loadingRecent')}</p>
        )}
        {(groups.error || directChats.error) && (
          <p className="text-xs text-destructive">{t('groups:recentLoadError')}</p>
        )}
        {recent.length > 0 && (
          <div className="w-full">
            <p className="pb-2 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
              {t('groups:recent')}
            </p>
            <ul className="space-y-1.5">
              {recent.map((conversation) => (
                <li key={`${conversation.kind}:${conversation.id}`}>
                  <Link
                    to={conversation.to}
                    className="flex items-center gap-3 rounded-lg border border-border bg-card px-4 py-3 transition-colors hover:bg-card-hover"
                  >
                    <Avatar className="h-8 w-8 shrink-0">
                      <AvatarFallback className={avatarColorClass(conversation.id)}>
                        {conversation.title.slice(0, 1).toUpperCase()}
                      </AvatarFallback>
                    </Avatar>
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-sm font-medium">{conversation.title}</p>
                      <p className="line-clamp-1 text-xs text-muted-foreground">
                        {conversation.subtitle}
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
