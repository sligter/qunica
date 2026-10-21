import { useEffect, useState, type ReactNode } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { ServerConnectionForm } from './ServerConnectionForm'
import { hasDesktopConnection, saveDesktopConnection, useDesktopConnection, type DesktopConnection } from '@/lib/desktopConnection'
import { useAuthStore } from '@/stores/authStore'
import { checkInstance } from '@/lib/realtime/instance'
import { DesktopInstanceContext } from '@/lib/desktopInstanceContext'

export function DesktopShell({ children }: { children: ReactNode }) {
  const { i18n } = useTranslation()
  const zh = i18n.language.startsWith('zh')
  const { connection } = useDesktopConnection()
  const [ready, setReady] = useState(false)
  const [selected, setSelected] = useState(hasDesktopConnection)
  const [editing, setEditing] = useState(() => !hasDesktopConnection())
  const [attempt, setAttempt] = useState(0)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    if (!selected) return
    let cancelled = false
    setReady(false)
    setError(null)
    const startup = connection.mode === 'local' ? invoke('backend_start') : checkInstance(connection.origin)
    void startup.then(() => { if (!cancelled) setReady(true) })
      .catch(cause => { if (!cancelled) { setError(String(cause)); setEditing(true) } })
    return () => { cancelled = true }
  }, [connection, selected, attempt])
  async function select(next: DesktopConnection) {
    const same = next.mode === connection.mode && (next.mode === 'local' || (connection.mode === 'remote' && next.origin === connection.origin))
    // Note: choosing the existing local instance must preserve its saved login.
    if (same) {
      saveDesktopConnection(connection)
      setSelected(true)
      setEditing(false)
      if (!ready) setAttempt(value => value + 1)
      return
    }
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
  return <DesktopInstanceContext.Provider value={{ select, add: () => setEditing(true) }}>
    {ready ? children : <main role="status" className="flex h-full items-center justify-center text-sm text-muted-foreground">{zh ? '正在连接实例…' : 'Connecting to instance…'}</main>}
  </DesktopInstanceContext.Provider>
}
