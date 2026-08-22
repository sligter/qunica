import { useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import { Folder, HardDrive, Plus, Users } from 'lucide-react'
import { Bot, Plus as PlusIcon } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { EntityCard } from '@/components/layout/EntityCard'
import { DetailShell } from '@/components/layout/DetailShell'
import { Button } from '@/components/ui/button'
import { SearchInput } from '@/components/ui/search-input'
import { useAgents } from '@/hooks/useAgents'
import { useGroups } from '@/hooks/useGroups'
import { useWorkspaces } from '@/hooks/useWorkspaces'
import { formatResourceStatus } from '@/i18n/resourceStatus'
import { avatarColorClass } from '@/lib/avatarColor'

export function WorkspacesIndexPage() {
  const { t } = useTranslation(['workspaces', 'common'])
  const workspaces = useWorkspaces()
  const groups = useGroups()
  const agents = useAgents()

  const list = workspaces.data ?? []
  const groupList = groups.data ?? []
  const agentList = agents.data ?? []
  const [query, setQuery] = useState('')
  // Paths are the main thing people search on here — a workspace is known by
  // where it points, not only by its display name.
  const listKey = list.map((w) => w.id).join(',')
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return list
    return list.filter((w) => {
      const target = w.backend_type === 'local' ? w.local_path : w.sandbox_ref
      return w.name.toLowerCase().includes(q) || target?.toLowerCase().includes(q)
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps -- list is stable per query cache entry; keying on ids avoids re-running the filter on unrelated renders
  }, [listKey, query])
  const localCount = list.filter((w) => w.backend_type === 'local').length
  const boundWorkspaces = new Set([
    ...groupList.map((g) => g.workspace_id).filter(Boolean),
    ...agentList.map((a) => a.workspace_id).filter(Boolean),
  ]).size

  return (
    <DetailShell
      title={t('workspaces:list.selectTitle')}
      subtitle={t('workspaces:list.selectDescription')}
      measure="wide"
      actions={
        <>
          {list.length > 0 ? (
            <SearchInput value={query} onChange={setQuery} label={t('workspaces:search')} />
          ) : null}
          <Button size="sm" asChild>
            <Link to="/workspaces/new">
              <Plus className="mr-1.5 h-3.5 w-3.5" />
              {t('workspaces:new')}
            </Link>
          </Button>
        </>
      }
    >
      <div className="space-y-6">
        {/* Metric Cards Row */}
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <div className="rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">{t('workspaces:title')}</span>
              <Folder className="h-4 w-4 text-primary/70" />
            </div>
            <p className="mt-2 text-2xl font-semibold tracking-tight">{list.length}</p>
          </div>
          <div className="rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">{t('workspaces:backendTypes.local')}</span>
              <HardDrive className="h-4 w-4 text-info/70" />
            </div>
            <p className="mt-2 text-2xl font-semibold tracking-tight text-info">{localCount}</p>
          </div>
          <div className="rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">{t('workspaces:detail.usedBy')}</span>
              <Users className="h-4 w-4 text-success/70" />
            </div>
            <p className="mt-2 text-2xl font-semibold tracking-tight text-success">{boundWorkspaces}</p>
          </div>
          <div className="rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">{t('workspaces:detail.agent')}</span>
              <Bot className="h-4 w-4 text-warning-foreground/70" />
            </div>
            <p className="mt-2 text-2xl font-semibold tracking-tight">{agentList.length}</p>
          </div>
        </div>

        {/* Gallery Grid or Empty State */}
        {list.length === 0 ? (
          <div className="flex flex-col items-center justify-center rounded-2xl border border-dashed border-border/80 bg-card/30 p-12 text-center">
            <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-2xl bg-primary/10 text-primary">
              <Folder className="h-6 w-6" />
            </div>
            <h3 className="text-base font-semibold">{t('workspaces:empty')}</h3>
            <p className="mt-1 max-w-sm text-sm text-muted-foreground">
              {t('workspaces:form.createSubtitle')}
            </p>
            <Button className="mt-6 gap-2" asChild>
              <Link to="/workspaces/new">
                <PlusIcon className="h-4 w-4" />
                {t('workspaces:new')}
              </Link>
            </Button>
          </div>
        ) : (
          <div className="grid grid-cols-1 gap-3.5 sm:grid-cols-2 xl:grid-cols-3">
            {filtered.map((workspace) => {
              const boundGroups = groupList.filter((g) => g.workspace_id === workspace.id)
              const boundAgents = agentList.filter((a) => a.workspace_id === workspace.id)
              const pathText =
                workspace.backend_type === 'local'
                  ? workspace.local_path || t('workspaces:noLocalPath')
                  : workspace.sandbox_ref || t('workspaces:noSandboxReference')
              const stats = [
                ...(boundGroups.length > 0
                  ? [{ key: 'groups', icon: Users, content: boundGroups.length }]
                  : []),
                ...(boundAgents.length > 0
                  ? [{ key: 'agents', icon: Bot, content: boundAgents.length }]
                  : []),
                ...(boundGroups.length === 0 && boundAgents.length === 0
                  ? [{ key: 'unused', content: t('workspaces:detail.unused') }]
                  : []),
              ]

              return (
                <EntityCard
                  key={workspace.id}
                  to={`/workspaces/${workspace.id}`}
                  // The workspace detail page edits inline — there is no
                  // separate edit form to deep-link into.
                  title={workspace.name}
                  avatarInitial={workspace.name.slice(0, 1).toUpperCase()}
                  avatarClass={avatarColorClass(workspace.id)}
                  statusLabel={formatResourceStatus(workspace.status, t)}
                  statusActive={workspace.status === 'active'}
                  metaBadge={{
                    label: workspace.backend_type,
                    className:
                      'border border-border/60 bg-muted/60 font-mono text-muted-foreground',
                  }}
                  description={<code className="truncate">{pathText}</code>}
                  stats={stats}
                />
              )
            })}
            {filtered.length === 0 ? (
              <p className="col-span-full py-12 text-center text-sm text-muted-foreground">
                {t('workspaces:noMatches', '没有匹配的工作区。')}
              </p>
            ) : null}
          </div>
        )}
      </div>
    </DetailShell>
  )
}
