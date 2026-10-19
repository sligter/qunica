import { useEffect, useRef, useState } from 'react'
import { Maximize2, RefreshCw } from 'lucide-react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import type RFB from '@novnc/novnc/lib/rfb'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from '@/components/ui/dialog'
import { fetchJson } from '@/lib/api-v2/client'
import { apiOrigin } from '@/lib/runtime'
import { LAN_ORIGIN } from '@/lib/androidSession'
import { abortOnAuthChange } from '@/lib/authFetch'
import { realtimeClient } from '@/lib/realtime/client'
import { useAuthStore } from '@/stores/authStore'

interface Control { state: string; generation: string; credential?: string | null; expires_at: number }
interface Lease { credential: string; token: string; origin: string; generation: string; computerId: string }

function releaseLease(id: string, lease: Lease | null) {
  if (!lease || lease.computerId !== id || apiOrigin() !== lease.origin || useAuthStore.getState().token !== lease.token) return
  void fetchJson(`/computers/${id}/control`, {
    method: 'POST', token: lease.token,
    body: { action: 'release', expected_generation: lease.generation, credential: lease.credential },
  }).catch(() => { /* The server lease still expires if a release cannot be delivered. */ })
}

export function ComputerDesktop({ id, name, generation, embedded = false }: { id: string; name: string; generation: string; embedded?: boolean }) {
  const { t } = useTranslation('computers')
  const token = useAuthStore(s => s.token)
  const queries = useQueryClient()
  const [open, setOpen] = useState(embedded)
  const [expanded, setExpanded] = useState(false)
  const [connection, setConnection] = useState(0)
  const [lease, setLease] = useState<Lease | null>(null)
  const ownedLease = useRef<Lease | null>(null)
  const mounted = useRef(false)
  const credential = lease?.token === token && lease.origin === apiOrigin() && lease.generation === generation && lease.computerId === id ? lease.credential : null
  const [error, setError] = useState('')
  const [connected, setConnected] = useState(false)
  const [viewport, setViewport] = useState<HTMLDivElement | null>(null)
  const control = useQuery({ queryKey: ['computer-control', id, generation], queryFn: () => fetchJson<Control>(`/computers/${id}/control`, { method: 'POST', token, body: { action: 'status', expected_generation: generation } }), enabled: open && !!token, refetchInterval: 5_000 })
  const mutation = useMutation({
    mutationFn: async (action: 'request' | 'release' | 'resume') => {
      const identity = { token, origin: apiOrigin(), generation, computerId: id }
      const result = await fetchJson<Control>(`/computers/${id}/control`, { method: 'POST', token, body: { action, expected_generation: generation, credential } })
      return { result, identity }
    },
    onSuccess: ({ result, identity }) => {
      const next = result.credential && identity.token ? { credential: result.credential, ...identity, token: identity.token } : null
      if (!mounted.current || identity.computerId !== id || identity.generation !== generation) { releaseLease(identity.computerId, next); return }
      if (useAuthStore.getState().token !== identity.token || apiOrigin() !== identity.origin) return
      ownedLease.current = next
      setLease(next)
      setError(''); void queries.invalidateQueries({ queryKey: ['computer-control', id] })
    },
    onError: error => setError(error.message),
  })
  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
      // Note: leaving an inline tab releases human control but never resumes
      // the agent. No control credential or input survives the view's lifetime.
      releaseLease(id, ownedLease.current)
      ownedLease.current = null
    }
  }, [id, generation])
  useEffect(() => {
    if (!open || !token || !viewport) return
    const controller = new AbortController()
    const unbind = abortOnAuthChange(controller, token)
    let socket: WebSocket | undefined
    let rfb: RFB | undefined
    setConnected(false)
    const connect = async () => {
      try {
        const { default: RFBClass } = await import('@novnc/novnc/lib/rfb')
        if (controller.signal.aborted) return
        const ticket = await realtimeClient(token).request<{ ticket: string }>('/realtime/tickets', { method: 'POST' })
        if (controller.signal.aborted) return
        const url = new URL(`${apiOrigin()}/api/v2/computers/desktop`, window.location.href)
        url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
        socket = new WebSocket(url, 'qunica.desktop.v1')
        socket.binaryType = 'arraybuffer'
        socket.onopen = () => socket?.send(JSON.stringify({ type: 'authenticate', ticket: ticket.ticket, computer_id: id, generation, credential }))
        socket.onmessage = message => {
          if (typeof message.data !== 'string' || controller.signal.aborted || !viewport) return
          if (JSON.parse(message.data).type !== 'authenticated') return
          socket!.onmessage = null
          if (!viewport || !socket) return
          rfb = new RFBClass(viewport, socket)
          rfb.viewOnly = !credential
          rfb.scaleViewport = true
          rfb.resizeSession = false
          rfb.addEventListener('connect', () => { setConnected(true); setError('') })
          rfb.addEventListener('disconnect', () => { if (!controller.signal.aborted) { setConnected(false); setError(t('desktopDisconnected')) } })
        }
        socket.onclose = () => { if (!controller.signal.aborted) { setConnected(false); setError(t('desktopDisconnected')) } }
      } catch (error) { if (!controller.signal.aborted) setError(error instanceof Error ? error.message : t('desktopDisconnected')) }
    }
    void connect()
    const stop = () => { rfb?.disconnect(); socket?.close() }
    controller.signal.addEventListener('abort', stop, { once: true })
    return () => { controller.abort(); unbind(); stop() }
  }, [open, id, generation, credential, token, t, viewport, connection])
  useEffect(() => {
    if (!credential || !open || !token) return
    const origin = apiOrigin()
    const renew = setInterval(() => {
      if (apiOrigin() !== origin || useAuthStore.getState().token !== token) { setLease(null); return }
      void fetchJson<Control>(`/computers/${id}/control`, { method: 'POST', token, body: { action: 'renew', expected_generation: generation, credential } }).catch(() => { setLease(null); setError(t('desktopDisconnected')) })
    }, 20_000)
    return () => { clearInterval(renew) }
  }, [credential, open, id, generation, token, t])
  if (apiOrigin() === LAN_ORIGIN) return <p className="text-xs text-muted-foreground">{t('desktopDirectOnly')}</p>
  const viewer = <div className="flex min-h-0 flex-col gap-3">
    <div className="flex flex-wrap items-center gap-2 text-xs">
      <span role="status" className="mr-auto text-muted-foreground">{t(`controlStates.${control.data?.state ?? 'agent_controlled'}`)} · {connected ? t('desktopConnected') : t('desktopConnecting')}</span>
      <Button size="icon" variant="ghost" className="h-7 w-7" aria-label={t('reconnectDesktop')} onClick={() => { setError(''); setConnection(n => n + 1) }}><RefreshCw className="h-3.5 w-3.5" aria-hidden /></Button>
      {embedded && !expanded ? <Button size="icon" variant="ghost" className="h-7 w-7" aria-label={t('expandDesktop')} onClick={() => setExpanded(true)}><Maximize2 className="h-3.5 w-3.5" aria-hidden /></Button> : null}
    </div>
    <div ref={setViewport} aria-label={t('desktopViewport')} className="aspect-[8/5] min-h-36 overflow-hidden rounded-md border border-border bg-black" />
    <div className="flex flex-wrap gap-2">
      {!credential && ['agent_controlled', 'released'].includes(control.data?.state ?? '') ? <Button size="sm" disabled={mutation.isPending} onClick={() => mutation.mutate('request')}>{t('takeControl')}</Button> : null}
      {credential ? <Button size="sm" variant="outline" disabled={mutation.isPending} onClick={() => mutation.mutate('release')}>{t('releaseControl')}</Button> : null}
      {control.data?.state === 'released' ? <Button size="sm" variant="outline" disabled={mutation.isPending} onClick={() => mutation.mutate('resume')}>{t('returnToAgent')}</Button> : null}
    </div>
    {error || control.isError ? <p role="alert" className="break-words text-xs text-destructive">{error || control.error?.message}</p> : null}
  </div>
  return <>
    {embedded ? (!expanded ? viewer : null) : <Button size="sm" variant="outline" onClick={() => setOpen(true)}>{t('openDesktop')}</Button>}
    <Dialog open={embedded ? expanded : open} onOpenChange={next => {
      if (embedded) setExpanded(next)
      else { if (!next && credential) mutation.mutate('release'); setOpen(next) }
    }}>
      <DialogContent className="flex max-h-[95dvh] w-[96vw] max-w-6xl flex-col gap-3 overflow-y-auto">
        <DialogHeader><DialogTitle>{name}</DialogTitle><DialogDescription>{t('desktopDescription')}</DialogDescription></DialogHeader>
        {viewer}
      </DialogContent>
    </Dialog>
  </>
}
