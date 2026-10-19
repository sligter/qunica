import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Monitor, Plus, RefreshCcw } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { DetailShell } from '@/components/layout/DetailShell'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { AlertDialog, AlertDialogContent, AlertDialogHeader, AlertDialogTitle, AlertDialogDescription, AlertDialogFooter, AlertDialogCancel, AlertDialogAction } from '@/components/ui/alert-dialog'
import { useAgents } from '@/hooks/useAgents'
import { useGroups } from '@/hooks/useGroups'
import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import { ComputerDesktop } from '@/components/computers/ComputerDesktop'
import { ComputerArtifacts } from '@/components/computers/ComputerArtifacts'

interface ComputerItem { id: string; name: string; binding: { kind: 'team' | 'private'; id: string }; selected: boolean }
interface ComputerState { generation: string; status: string; home_present: boolean; error_code: string | null; capabilities?: { desktop: boolean } }

function ComputerCard({ item, targetName }: { item: ComputerItem; targetName: string }) {
  const { t } = useTranslation('computers')
  const token = useAuthStore((s) => s.token)
  const queries = useQueryClient()
  const [confirmation, setConfirmation] = useState<'rebuild' | 'destroy' | null>(null)
  const [purge, setPurge] = useState(false)
  const state = useQuery({ queryKey: ['computers', item.id], queryFn: () => fetchJson<ComputerState>(`/computers/${item.id}`, { token }), refetchInterval: 5_000 })
  const mutation = useMutation({
    mutationFn: async (action: 'start' | 'stop' | 'rebuild' | 'destroy' | 'bind' | 'unbind') => {
      if (action === 'bind' || action === 'unbind') {
        await fetchJson('/execution-environments/binding', { method: 'PUT', token, body: { scope: item.binding.kind === 'team' ? 'group' : 'agent', target_id: item.binding.id, execution_environment_id: action === 'bind' ? item.id : null } })
      } else {
        if (!state.data) throw new Error(t('refreshRequired'))
        await fetchJson(`/computers/${item.id}/actions`, { method: 'POST', token, body: { action, expected_generation: state.data.generation, confirm: action === 'destroy' ? item.id : null, purge_home: action === 'destroy' && purge } })
      }
    },
    onSettled: () => {
      void queries.invalidateQueries({ queryKey: ['computers'] })
      void queries.invalidateQueries({ queryKey: ['groups'] })
      void queries.invalidateQueries({ queryKey: ['agents'] })
    },
  })
  const status = state.data?.status
  const busy = mutation.isPending || !state.data || ['starting', 'executing', 'stopping', 'rebuilding', 'destroying'].includes(status ?? '')
  return <article className="rounded-lg border border-border bg-card p-4">
    <div className="flex items-start gap-3">
      <span className="rounded-lg bg-muted p-2"><Monitor className="h-5 w-5 text-muted-foreground" aria-hidden="true" /></span>
      <div className="min-w-0 flex-1">
        <h2 className="truncate font-medium">{item.name}</h2>
        <p className="mt-0.5 truncate text-xs text-muted-foreground">{t(item.binding.kind)} · {targetName}</p>
      </div>
      <span className="rounded border border-border px-2 py-0.5 text-xs">{state.data ? t(`statuses.${status}`, { defaultValue: status }) : t('loading')}</span>
    </div>
    <dl className="mt-4 grid grid-cols-2 gap-2 text-xs">
      <div><dt className="text-muted-foreground">{t('generation')}</dt><dd className="mt-1 font-mono">{state.data?.generation ?? '—'}</dd></div>
      <div><dt className="text-muted-foreground">{t('execution')}</dt><dd className="mt-1">{item.selected ? t('selected') : t('unselected')}</dd></div>
    </dl>
    <p className="mt-3 truncate font-mono text-2xs text-muted-foreground" title={item.id}>{item.id}</p>
    {state.isError ? <p role="alert" className="mt-2 break-words text-xs text-destructive">{state.error.message}</p> : null}
    {state.data?.error_code ? <p className="mt-2 break-words text-xs text-destructive">{state.data.error_code}</p> : null}
    {mutation.isError ? <p role="alert" className="mt-2 break-words text-xs text-destructive">{mutation.error.message}</p> : null}
    <div className="mt-4 flex flex-wrap gap-2 border-t border-border pt-3">
      {status === 'running' && state.data?.capabilities?.desktop ? <ComputerDesktop id={item.id} name={item.name} generation={state.data.generation} /> : null}
      <Button size="sm" variant="outline" disabled={busy || status === 'destroyed' || (!item.selected && status !== 'running')} onClick={() => mutation.mutate(item.selected ? 'unbind' : 'bind')}>{item.selected ? t('unbind') : t('bind')}</Button>
      {status === 'stopped' ? <Button size="sm" variant="outline" disabled={busy} onClick={() => mutation.mutate('start')}>{t('start')}</Button> : null}
      {status !== 'stopped' && status !== 'destroyed' ? <Button size="sm" variant="outline" disabled={busy} onClick={() => mutation.mutate('stop')}>{t('stop')}</Button> : null}
      <Button size="sm" variant="ghost" disabled={busy || !['running', 'stopped'].includes(status ?? '')} onClick={() => setConfirmation('rebuild')}>{t('rebuild')}</Button>
      <Button size="sm" variant="ghost" className="text-destructive" disabled={busy || status === 'destroyed'} onClick={() => { setPurge(false); setConfirmation('destroy') }}>{t('destroy')}</Button>
      <Button size="sm" variant="ghost" className="ml-auto" aria-label={t('refresh')} onClick={() => void state.refetch()}><RefreshCcw className="h-3.5 w-3.5" aria-hidden="true" /></Button>
    </div>
    <ComputerArtifacts id={item.id} />
    <AlertDialog open={confirmation !== null} onOpenChange={(open) => { if (!open) setConfirmation(null) }}>
      <AlertDialogContent>
        <AlertDialogHeader><AlertDialogTitle>{t(confirmation === 'destroy' ? 'destroy' : 'rebuild')} · {item.name}</AlertDialogTitle><AlertDialogDescription>{t(confirmation === 'destroy' ? 'destroyDescription' : 'rebuildDescription')}</AlertDialogDescription></AlertDialogHeader>
        {confirmation === 'destroy' ? <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={purge} onChange={(e) => setPurge(e.target.checked)} />{t('purge')}</label> : null}
        <AlertDialogFooter><AlertDialogCancel>{t('cancel')}</AlertDialogCancel><AlertDialogAction onClick={() => { if (confirmation) mutation.mutate(confirmation) }}>{t('confirm')}</AlertDialogAction></AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  </article>
}

export function ComputersPage() {
  const { t } = useTranslation('computers')
  const token = useAuthStore((s) => s.token)
  const queries = useQueryClient()
  const groups = useGroups()
  const agents = useAgents()
  const computers = useQuery({ queryKey: ['computers'], queryFn: () => fetchJson<{ configured: boolean; items: ComputerItem[] }>('/computers', { token }), enabled: !!token })
  const [name, setName] = useState('')
  const [kind, setKind] = useState<'team' | 'private'>('team')
  const [target, setTarget] = useState('')
  // Keep this intent stable after a lost create response. A retry must read the
  // same Supervisor record, rather than provisioning a second computer.
  const [intent, setIntent] = useState(() => crypto.randomUUID())
  const targets = kind === 'team' ? (groups.data ?? []) : (agents.data ?? []).filter((a) => a.runtime_kind !== 'acp' || a.acp_runtime?.profile === 'opencode')
  const create = useMutation({
    mutationFn: () => fetchJson('/computers', { method: 'POST', token, body: { id: intent, name: name.trim(), binding: { kind, id: target } } }),
    onSuccess: () => { setName(''); setIntent(crypto.randomUUID()); void queries.invalidateQueries({ queryKey: ['computers'] }) },
  })
  return <DetailShell title={t('title')} subtitle={t('description')}>
    <div className="space-y-6 py-5">
      {computers.isError ? <p role="alert" className="text-sm text-destructive">{computers.error.message}</p> : null}
      {computers.data?.configured === false ? <p role="status" className="rounded-lg border border-border bg-muted/40 p-4 text-sm text-muted-foreground">{t('notConfigured')}</p> : null}
      <form className="space-y-4 rounded-lg border border-border p-4" onSubmit={(event) => { event.preventDefault(); create.mutate() }}>
        <h2 className="text-sm font-medium">{t('create')}</h2>
        <div className="grid gap-4 sm:grid-cols-3">
          <div className="space-y-2"><Label htmlFor="computer-name">{t('name')}</Label><Input id="computer-name" value={name} maxLength={120} onChange={(e) => { setName(e.target.value); if (create.isError) { setIntent(crypto.randomUUID()); create.reset() } }} required /></div>
          <div className="space-y-2"><Label htmlFor="computer-kind">{t('scope')}</Label><select id="computer-kind" className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm" value={kind} onChange={(e) => { setKind(e.target.value as typeof kind); setTarget(''); setIntent(crypto.randomUUID()); create.reset() }}><option value="team">{t('team')}</option><option value="private">{t('private')}</option></select></div>
          <div className="space-y-2"><Label htmlFor="computer-target">{t('target')}</Label><select id="computer-target" className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm" value={target} required onChange={(e) => { setTarget(e.target.value); setIntent(crypto.randomUUID()); create.reset() }}><option value="">{t('choose')}</option>{targets.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</select></div>
        </div>
        <p className="text-xs leading-relaxed text-muted-foreground">{t('capabilities')}</p>
        {create.isError ? <p role="alert" className="text-xs text-destructive">{create.error.message}</p> : null}
        <Button size="sm" disabled={!computers.data?.configured || create.isPending || !name.trim() || !target} type="submit"><Plus className="h-3.5 w-3.5" aria-hidden="true" />{create.isPending ? t('creating') : t('create')}</Button>
      </form>
      {computers.isLoading ? <p role="status" className="text-sm text-muted-foreground">{t('loading')}</p> : null}
      <div className="grid gap-4 lg:grid-cols-2">{computers.data?.items.map((item) => <ComputerCard key={item.id} item={item} targetName={(item.binding.kind === 'team' ? groups.data : agents.data)?.find((target) => target.id === item.binding.id)?.name ?? item.binding.id} />)}</div>
    </div>
  </DetailShell>
}
