import { useEffect } from 'react'
import { ChevronRight, NotebookPen, Settings2, UsersRound, X } from 'lucide-react'
import { useParams, useSearchParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'

import { AgentAvatar } from '@/components/chat/AgentAvatar'
import { GroupNotesPanel } from '@/components/chat/GroupNotesPanel'
import {
  GroupAvatarEditor,
  type GroupAvatarMember,
} from '@/components/groups/GroupAvatar'
import { useCloseOverlay } from '@/components/layout/overlayRouting'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { PageState } from '@/components/ui/page-state'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useGroupAgents } from '@/hooks/useGroupAgents'
import { useGroupMembers } from '@/hooks/useGroupMembers'
import { useGroup } from '@/hooks/useGroups'
import { normalizeLanguage } from '@/i18n'
import { formatNumber } from '@/lib/format'
import { GroupMembersTab } from '@/pages/group/GroupMembersTab'
import { GroupSettingsTab } from '@/pages/group/GroupSettingsTab'
import type { GroupRead } from '@/types/api'

type ManageTab = 'settings' | 'members' | 'notes'
const MEMBER_PREVIEW_LIMIT = 5

function parseTab(value: string | null): ManageTab {
  return value === 'members' || value === 'notes' ? value : 'settings'
}

function GroupOverview({
  group,
  onOpenMembers,
}: {
  group: GroupRead
  onOpenMembers: () => void
}) {
  const { t, i18n } = useTranslation('groups')
  const language = normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US'
  const humans = useGroupMembers(group.id)
  const agents = useGroupAgents(group.id)
  const roster: GroupAvatarMember[] = [
    ...(humans.data ?? []).map((member) => ({
      id: `human:${member.user_id}`,
      name: member.display_name,
      kind: 'user' as const,
      avatarUrl: member.avatar_url,
    })),
    ...(agents.data ?? []).map((agent) => ({
      id: `agent:${agent.agent_id}`,
      name: agent.display_name,
      kind: 'agent' as const,
      avatarUrl: agent.avatar_url,
    })),
  ]
  const loading = humans.isLoading || agents.isLoading
  const preview = roster.slice(0, MEMBER_PREVIEW_LIMIT)
  const remaining = Math.max(0, roster.length - preview.length)

  return (
    <Card asChild className="overflow-hidden shadow-xs">
      <section aria-labelledby="group-overview-name">
        <div className="flex items-center gap-3 p-3.5">
          <GroupAvatarEditor
            groupId={group.id}
            name={group.name}
            avatarUrl={group.avatar_url}
            members={roster}
          />
          <div className="min-w-0">
            <h2 id="group-overview-name" className="truncate text-base font-semibold tracking-tight">
              {group.name}
            </h2>
            <p className="mt-0.5 truncate text-xs leading-5 text-muted-foreground">
              {group.announcement || group.description || t('settings.basicDescription')}
            </p>
          </div>
        </div>

        <button
          type="button"
          className="group flex w-full items-center gap-3 border-t border-border px-3.5 py-3 text-left outline-none transition-colors hover:bg-card-hover focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
          onClick={onOpenMembers}
          aria-label={t('manage.members')}
        >
          <span className="min-w-0 flex-1">
            <span className="block text-sm font-semibold">{t('manage.members')}</span>
            <span className="mt-0.5 block text-xs text-muted-foreground">
              {loading
                ? t('members.loading')
                : t('manage.memberCount', {
                    count: roster.length,
                    formattedCount: formatNumber(roster.length, language),
                  })}
            </span>
          </span>

          <span className="flex shrink-0 items-center" aria-hidden="true">
            {loading ? (
              <span className="flex -space-x-2">
                {Array.from({ length: 3 }, (_, index) => (
                  <span
                    key={index}
                    className="h-8 w-8 animate-pulse rounded-full border-2 border-card bg-muted"
                  />
                ))}
              </span>
            ) : preview.length > 0 ? (
              <span className="flex -space-x-2">
                {preview.map((entry) => (
                  <AgentAvatar
                    key={entry.id}
                    name={entry.name}
                    kind={entry.kind}
                    avatarUrl={entry.avatarUrl}
                    size="md"
                    className="rounded-full ring-2 ring-card"
                  />
                ))}
              </span>
            ) : (
              <span className="flex h-8 w-8 items-center justify-center rounded-full bg-muted text-muted-foreground">
                <UsersRound className="h-4 w-4" />
              </span>
            )}
            {!loading && remaining > 0 ? (
              <span
                className="relative z-10 -ml-2 flex h-8 min-w-8 items-center justify-center rounded-full border-2 border-card bg-muted px-1.5 text-2xs font-semibold text-muted-foreground"
                title={t('members.count', {
                  count: roster.length,
                  formattedCount: formatNumber(roster.length, language),
                })}
              >
                +{formatNumber(remaining, language)}
              </span>
            ) : null}
          </span>
          <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5" />
        </button>
      </section>
    </Card>
  )
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
    <div className="flex h-full min-h-0 w-full flex-col overflow-hidden bg-muted/35">
      <header className="flex min-h-16 shrink-0 items-center justify-between gap-4 border-b border-border bg-background/95 px-5 py-3 backdrop-blur">
        <div className="min-w-0">
          <h1 className="font-serif text-base font-semibold tracking-tight">{t('manage.title')}</h1>
          {group.data?.name ? (
            <p className="mt-0.5 truncate text-xs text-muted-foreground">{group.data.name}</p>
          ) : null}
        </div>
        <Button
          variant="ghost"
          size="icon"
          className="h-9 w-9 rounded-full"
          aria-label={t('manage.back')}
          onClick={closeOverlay}
        >
          <X className="h-4 w-4" />
        </Button>
      </header>

      {group.error ? (
        <PageState
          inset
          variant="error"
          title={t('manage.loadErrorDetail', { message: String(group.error) })}
        />
      ) : group.isLoading ? (
        <PageState inset variant="loading" title={t('manage.loading')} />
      ) : group.data ? (
        <Tabs value={tab} onValueChange={onTabChange} className="flex min-h-0 flex-1 flex-col">
          <TabsList
            variant="underline"
            className="h-12 shrink-0 gap-0 border-b border-border bg-background px-4"
          >
            <TabsTrigger value="settings" className="h-12 flex-1 gap-1.5 px-2">
              <Settings2 className="h-4 w-4" aria-hidden />
              {t('manage.settings')}
            </TabsTrigger>
            <TabsTrigger value="members" className="h-12 flex-1 gap-1.5 px-2">
              <UsersRound className="h-4 w-4" aria-hidden />
              {t('manage.members')}
            </TabsTrigger>
            <TabsTrigger value="notes" className="h-12 flex-1 gap-1.5 px-2">
              <NotebookPen className="h-4 w-4" aria-hidden />
              {t('manage.notes')}
            </TabsTrigger>
          </TabsList>

          <TabsContent value="settings" className="m-0 min-h-0 flex-1 overflow-y-auto p-4 pb-8">
            <div className="space-y-4">
              <GroupOverview group={group.data} onOpenMembers={() => onTabChange('members')} />
              <GroupSettingsTab group={group.data} compact />
            </div>
          </TabsContent>
          <TabsContent value="members" className="m-0 min-h-0 flex-1 overflow-y-auto p-4 pb-8">
            <GroupMembersTab groupId={groupId} compact />
          </TabsContent>
          <TabsContent value="notes" className="m-0 min-h-0 flex-1 overflow-y-auto p-4 pb-8">
            <Card className="w-full p-4 shadow-xs">
              <GroupNotesPanel groupId={groupId} />
            </Card>
          </TabsContent>
        </Tabs>
      ) : (
        <PageState inset title={t('manage.notFound')} />
      )}
    </div>
  )
}
