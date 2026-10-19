import { useEffect, useState, type ReactNode } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { ServerConnectionForm } from './ServerConnectionForm'
import { saveDesktopConnection, useDesktopConnection, type DesktopConnection } from '@/lib/desktopConnection'
import { useAuthStore } from '@/stores/authStore'
import { checkInstance } from '@/lib/realtime/instance'

export function DesktopShell({ children }: { children: ReactNode }) {
  const { i18n } = useTranslation()
  const zh = i18n.language.startsWith('zh')
  const { connection } = useDesktopConnection()
  const [ready, setReady] = useState(false)
  const [editing, setEditing] = useState(false)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    let cancelled = false
    const startup = connection.mode === 'local' ? invoke('backend_start') : checkInstance(connection.origin)
    void startup.then(() => { if (!cancelled) setReady(true) })
      .catch(cause => { if (!cancelled) { setError(String(cause)); setEditing(true) } })
    return () => { cancelled = true }
  }, [connection])
  async function select(next: DesktopConnection) {
    // Note: clear credentials and callback owners before changing the origin.
    useAuthStore.getState().logout()
    saveDesktopConnection(next)
    window.location.replace('/')
  }
  if (editing) return <main className="flex h-full items-center justify-center overflow-y-auto bg-background p-8"><div className="w-full max-w-md space-y-6">
    <ServerConnectionForm onConnect={origin => select({ mode: 'remote', origin })} onLocal={() => select({ mode: 'local' })} />
    {error && <p role="alert" className="text-xs text-destructive">{error}</p>}
    {ready && <Button variant="ghost" className="w-full" onClick={() => setEditing(false)}>{zh ? '返回工作台' : 'Back to workspace'}</Button>}
  </div></main>
  return <>
    <div className="flex shrink-0 items-center justify-between gap-3 border-b border-border bg-card px-4 py-1 text-xs">
      <span role="status" className="truncate text-muted-foreground">{connection.mode === 'remote' ? connection.origin : ready ? (zh ? '本地实例' : 'Local instance') : (zh ? '正在启动本地实例…' : 'Starting local instance…')}</span>
      <Button variant="ghost" size="sm" onClick={() => setEditing(true)}>{zh ? '切换实例' : 'Switch instance'}</Button>
    </div>
    {ready && children}
  </>
}
