import { useEffect, useState, type ReactNode } from 'react'
import { Cable, ChevronLeft, Loader2, Server, ShieldCheck } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { changeAndroidServer, initializeAndroidSession, retryAndroidPersistence, useAndroidSession } from '@/lib/androidSession'
import { useAuthStore } from '@/stores/authStore'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

export function AndroidShell({ children }: { children: ReactNode }) {
  const { i18n } = useTranslation()
  const zh = i18n.language.startsWith('zh')
  const { server, ready, error } = useAndroidSession()
  const token = useAuthStore(s => s.token)
  const [editing, setEditing] = useState(false)
  const [address, setAddress] = useState(server ?? '')
  const [busy, setBusy] = useState(false)
  const [connectionError, setConnectionError] = useState<string | null>(null)
  const initialize = () => {
    void initializeAndroidSession().then(token => useAuthStore.setState({ token })).catch(() => undefined)
  }
  useEffect(initialize, [])

  async function connect(event: React.FormEvent) {
    event.preventDefault()
    setBusy(true); setConnectionError(null)
    try {
      // Validate and probe before changing the authenticated server. Never send an old token here.
      const { normalizeServerOrigin } = await import('@/lib/androidSession')
      const origin = normalizeServerOrigin(address)
      const response = await fetch(`${origin}/api/v2/health`, { cache: 'no-store', signal: AbortSignal.timeout(10_000) })
      if (!response.ok) throw new Error(`HTTP ${response.status}`)
      const health = await response.json() as { service?: string }
      if (health.service !== 'qunica-backend') throw new Error(zh ? '该地址不是 Qunica Server' : 'This address is not a Qunica Server')
      useAuthStore.getState().logout()
      await changeAndroidServer(origin)
      window.history.replaceState(null, '', '/')
      // Rebuild the router and all query scopes against the newly saved origin.
      window.location.reload()
    } catch (cause) {
      setConnectionError(zh ? `连接失败：${String(cause)}。请检查 HTTPS 证书、网络和服务器 CORS 配置。` : `Connection failed: ${String(cause)}. Check HTTPS, connectivity and server CORS settings.`)
    } finally { setBusy(false) }
  }

  if (!ready || !server || editing) return (
    <main className="app-safe-area flex h-full min-h-0 flex-col overflow-y-auto bg-background px-6 py-8">
      <header className="mb-12 flex items-center justify-between">
        <span className="font-serif text-2xl font-semibold">Qunica<span className="ml-2 text-xs font-sans font-normal text-muted-foreground">Android</span></span>
        {server && ready ? <Button variant="ghost" size="icon" aria-label={zh ? '返回' : 'Back'} onClick={() => setEditing(false)}><ChevronLeft /></Button> : null}
      </header>
      <div className="mx-auto w-full max-w-md flex-1">
        <div className="mb-6 flex h-14 w-14 items-center justify-center rounded-2xl border border-border bg-muted text-primary"><Cable size={26} /></div>
        <p className="mb-3 text-xs tracking-[0.2em] text-primary">YOUR WORKSPACE, ANYWHERE</p>
        <h1 className="mb-3 font-serif text-3xl leading-tight">{zh ? '连接你的工作台' : 'Connect your workspace'}</h1>
        <p className="mb-8 text-sm leading-6 text-muted-foreground">{zh ? '任务在服务器运行。用手机接收回复、处理审批，随时回到同一段对话。' : 'Tasks run on your server. Follow replies, handle approvals and return to the same conversation from your phone.'}</p>
        {!ready ? (
          <div role="status" className="space-y-3 text-sm">
            {error ? <><p>{zh ? '无法读取安全存储，请重试。' : 'Unable to read secure storage. Try again.'}</p><Button onClick={initialize}>{zh ? '重试' : 'Retry'}</Button></> : <Loader2 className="animate-spin" />}
          </div>
        ) : (
          <form onSubmit={connect} className="space-y-4">
            <label htmlFor="android-server" className="block text-sm font-medium">{zh ? '服务器地址' : 'Server address'}</label>
            <Input id="android-server" type="url" inputMode="url" autoCapitalize="none" autoCorrect="off" placeholder="https://qunica.example.com" value={address} onChange={event => setAddress(event.target.value)} required disabled={busy} className="h-12 text-base" />
            <p className="text-xs leading-5 text-muted-foreground">{zh ? '使用已部署的 HTTPS 地址，或可信 VPN 内的 HTTPS 地址。切换服务器后需要重新登录。' : 'Use your deployed HTTPS address, including HTTPS through a trusted VPN. Changing servers signs you out.'}</p>
            {connectionError ? <p role="alert" className="text-sm text-destructive">{connectionError}</p> : null}
            <Button type="submit" disabled={busy} className="h-12 w-full gap-2">{busy ? <Loader2 className="h-4 w-4 animate-spin" /> : <Server className="h-4 w-4" />}{zh ? '连接服务器' : 'Connect to server'}</Button>
          </form>
        )}
      </div>
      <p className="mx-auto mt-10 flex max-w-md items-center gap-2 text-xs text-muted-foreground"><ShieldCheck className="h-4 w-4 shrink-0" />{zh ? '登录凭据使用 Android Keystore 加密保存' : 'Credentials are encrypted using Android Keystore'}</p>
    </main>
  )

  return <>
    {!token ? <div className="app-safe-area flex shrink-0 items-center justify-between gap-2 border-b border-border bg-background px-3 py-1 text-xs"><span className="truncate text-muted-foreground">{server}</span><Button variant="ghost" size="sm" onClick={() => { setAddress(server); setEditing(true) }}>{zh ? '切换服务器' : 'Change server'}</Button></div> : null}
    {error ? <div role="alert" className="shrink-0 bg-destructive/10 px-3 py-2 text-sm">{zh ? '安全存储写入失败，请重试后再关闭应用。' : 'Secure storage failed. Retry before closing the app.'}<Button variant="ghost" onClick={() => { void retryAndroidPersistence().catch(() => undefined) }}>{zh ? '重试' : 'Retry'}</Button></div> : null}
    {children}
  </>
}
