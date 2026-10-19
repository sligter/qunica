import { useState } from 'react'
import { Link } from 'react-router-dom'
import { Monitor, Play, RefreshCw, Settings2, Square } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { ComputerArtifacts } from '@/components/computers/ComputerArtifacts'
import { ComputerDesktop } from '@/components/computers/ComputerDesktop'
import { Button } from '@/components/ui/button'
import { PageState } from '@/components/ui/page-state'
import { useComputers, useComputerState, useComputerAction } from '@/hooks/useComputers'
import { useDirectChat } from '@/hooks/useDirectChats'
import { useGroupAgents } from '@/hooks/useGroupAgents'
import type { ComputerItem, ConversationScope } from '@/types/api'

function ComputerView({ item }: { item: ComputerItem }) {
  const { t } = useTranslation('computers')
  const state = useComputerState(item.id)
  const action = useComputerAction(item, state.data?.generation)
  const status = state.data?.status
  // An in-flight command uses the same desktop generation. Keep observing it
  // while lifecycle actions remain disabled, so takeover can wait for its exit.
  const running = status === 'running' || status === 'executing'
  const busy = action.isPending || !state.data || !['running', 'stopped'].includes(status ?? '')

  return <>
    <div className="flex items-center gap-2 px-3 py-2 text-xs">
      <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${running ? 'bg-emerald-500' : 'bg-muted-foreground/50'}`} aria-hidden />
      <span role="status" className="min-w-0 flex-1 truncate text-muted-foreground">
        {state.data ? t(`statuses.${status}`, { defaultValue: status }) : t('loading')}
        {item.selected ? ` · ${t('selected')}` : ''}
      </span>
      {status === 'stopped' ? <Button size="sm" variant="ghost" className="h-7 gap-1 px-2" disabled={busy} onClick={() => action.mutate('start')}><Play className="h-3 w-3" aria-hidden />{t('start')}</Button> : null}
      {running ? <Button size="sm" variant="ghost" className="h-7 gap-1 px-2" disabled={busy} onClick={() => action.mutate('stop')}><Square className="h-3 w-3" aria-hidden />{t('stop')}</Button> : null}
      <Button size="icon" variant="ghost" className="h-7 w-7" aria-label={t('refresh')} onClick={() => void state.refetch()} disabled={state.isFetching}><RefreshCw className="h-3.5 w-3.5" aria-hidden /></Button>
    </div>
    <div className="space-y-3 px-3 pb-4">
      {state.isError || action.isError ? <p role="alert" className="break-words text-xs text-destructive">{state.error?.message || action.error?.message}</p> : null}
      {state.data?.error_code ? <p role="alert" className="break-words text-xs text-destructive">{state.data.error_code}</p> : null}
      {running && state.data?.capabilities?.desktop ? <ComputerDesktop key={`${item.id}:${state.data.generation}`} embedded id={item.id} name={item.name} generation={state.data.generation} /> : (
        <div className="flex aspect-[8/5] flex-col items-center justify-center gap-2 rounded-lg border border-dashed border-border bg-muted/20 px-4 text-center">
          <Monitor className="h-8 w-8 text-muted-foreground/60" aria-hidden />
          <p className="text-xs font-medium">{t(status === 'stopped' ? 'panel.stoppedTitle' : 'panel.noDesktopTitle')}</p>
          <p className="max-w-xs text-xs leading-relaxed text-muted-foreground">{t(status === 'stopped' ? 'panel.stoppedBody' : 'panel.noDesktopBody')}</p>
        </div>
      )}
      {!item.selected && status !== 'destroyed' ? <Button size="sm" variant="outline" className="w-full" disabled={busy || !running} onClick={() => action.mutate('bind')}>{t('bind')}</Button> : null}
      <ComputerArtifacts key={item.id} id={item.id} />
    </div>
  </>
}

export function ConversationComputerTab({ scope, conversationId }: { scope: ConversationScope; conversationId: string | undefined }) {
  const { t } = useTranslation('computers')
  const computers = useComputers(!!conversationId)
  const members = useGroupAgents(scope === 'groups' ? conversationId : undefined)
  const direct = useDirectChat(scope === 'direct-chats' ? conversationId : undefined)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const agentIds = new Set(scope === 'groups' ? (members.data ?? []).filter(a => a.status === 'active').map(a => a.agent_id) : direct.data?.agent_id ? [direct.data.agent_id] : [])
  // Note: the account list is shared with settings; only this conversation's
  // Team computer and member-owned Private computers belong in its inspector.
  const items = (computers.data?.items ?? []).filter(item => item.binding.kind === 'team'
    ? scope === 'groups' && item.binding.id === conversationId
    : agentIds.has(item.binding.id))
  const selected = items.find(item => item.id === selectedId)
    ?? items.find(item => item.selected && item.binding.kind === 'team')
    ?? items.find(item => item.selected)
    ?? items[0]
  const contextError = scope === 'groups' ? members.error : direct.error
  const manage = <Button asChild size="sm" variant="outline"><Link to="/settings/computers"><Settings2 className="h-3.5 w-3.5" aria-hidden />{t('panel.manage')}</Link></Button>

  if (!conversationId) return <PageState icon={Monitor} title={t('panel.noConversation')} className="p-5" />
  if (computers.isLoading || (!selected && (scope === 'groups' ? members.isLoading : direct.isLoading))) return <PageState variant="loading" title={t('loading')} className="p-5" />
  if (computers.error) return <PageState variant="error" title={t('panel.loadError')} description={computers.error.message} className="p-5" action={<Button size="sm" variant="outline" onClick={() => void computers.refetch()}>{t('refresh')}</Button>} />
  if (computers.data?.configured === false) return <PageState icon={Monitor} title={t('panel.notConfiguredTitle')} description={t('notConfigured')} action={manage} className="p-5 [&_p]:text-xs" />

  return <div className="flex h-full min-h-0 flex-col">
    <div className="flex min-h-12 shrink-0 items-center gap-2 border-b border-border px-3 py-2">
      <Monitor className="h-4 w-4 shrink-0 text-muted-foreground" aria-hidden />
      <div className="min-w-0 flex-1">
        {selected ? <select aria-label={t('panel.choose')} value={selected.id} onChange={event => setSelectedId(event.target.value)} className="h-7 w-full min-w-0 truncate rounded-md border-0 bg-transparent pr-2 text-xs font-medium outline-none focus-visible:ring-2 focus-visible:ring-ring">
          {items.map(item => <option key={item.id} value={item.id}>{item.name} · {t(item.binding.kind)}</option>)}
        </select> : <p className="text-xs font-medium">{t('title')}</p>}
      </div>
      <Button asChild size="icon" variant="ghost" className="h-7 w-7" aria-label={t('panel.manage')}><Link to="/settings/computers"><Settings2 className="h-3.5 w-3.5" aria-hidden /></Link></Button>
    </div>
    <div className="min-h-0 flex-1 overflow-y-auto">
      {contextError ? <p role="alert" className="px-3 pt-3 text-xs text-destructive">{contextError.message}</p> : null}
      {selected ? <ComputerView key={selected.id} item={selected} /> : <PageState icon={Monitor} title={t('panel.emptyTitle')} description={t('panel.emptyBody')} action={manage} className="min-h-64 p-5 [&_p]:text-xs" />}
    </div>
  </div>
}
