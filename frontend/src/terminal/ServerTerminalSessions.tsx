import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import type { WireDescriptor } from './httpTransport'

export function ServerTerminalSessions({ conversationId, onAttach, onClose }: {
  conversationId: string
  onAttach: (sessionId: string) => Promise<void>
  onClose: () => void
}) {
  const { t } = useTranslation('chat')
  const token = useAuthStore(state => state.token)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const sessions = useQuery({ queryKey: ['terminal-sessions', conversationId],
    queryFn: () => fetchJson<WireDescriptor[]>(`/terminal/sessions?conversation_id=${encodeURIComponent(conversationId)}`, { token }), enabled: !!token })
  return <Dialog open onOpenChange={open => { if (!open) onClose() }}><DialogContent closeLabel={t('common:actions.close')} className="sm:max-w-lg">
    <DialogHeader><DialogTitle>{t('terminal.serverSessions')}</DialogTitle><DialogDescription>{t('terminal.serverSessionsHelp')}</DialogDescription></DialogHeader>
    <div className="max-h-80 space-y-2 overflow-y-auto">
      {sessions.isLoading && <p role="status" className="text-sm text-muted-foreground">{t('common:state.loading')}</p>}
      {sessions.data?.length === 0 && <p className="text-sm text-muted-foreground">{t('terminal.noServerSessions')}</p>}
      {sessions.data?.map(session => <button key={session.session_id} disabled={busy} className="w-full rounded-lg border border-border px-3 py-3 text-left transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50" onClick={() => {
        setBusy(true); setError(null)
        void onAttach(session.session_id).then(onClose).catch(cause => setError(String(cause))).finally(() => setBusy(false))
      }}><span className="flex items-center justify-between gap-3 text-sm font-medium"><span>{session.shell_name}</span><span className="font-mono text-xs text-muted-foreground">{session.session_id.slice(0, 8)}</span></span><span className="mt-1 block truncate font-mono text-xs text-muted-foreground">{session.cwd}</span></button>)}
      {(sessions.error || error) && <p role="alert" className="text-sm text-destructive">{error ?? String(sessions.error)}</p>}
    </div>
    <Button variant="outline" disabled={busy || sessions.isFetching} onClick={() => void sessions.refetch()}>{t('workspace.refresh')}</Button>
  </DialogContent></Dialog>
}
