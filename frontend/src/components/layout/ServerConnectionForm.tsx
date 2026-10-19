import { useState } from 'react'
import { ArrowRight, Loader2, Monitor, Server } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { normalizeServerOrigin } from '@/lib/androidSession'

export function ServerConnectionForm({ onConnect, onLocal }: {
  onConnect: (origin: string) => Promise<void>
  onLocal?: () => Promise<void>
}) {
  const { i18n } = useTranslation()
  const zh = i18n.language.startsWith('zh')
  const [address, setAddress] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  async function connect(local = false) {
    setBusy(true); setError(null)
    try {
      if (local) { await onLocal?.(); return }
      const origin = normalizeServerOrigin(address)
      await onConnect(origin)
    } catch (cause) { setError(zh ? `无法连接：${String(cause)}` : `Unable to connect: ${String(cause)}`) }
    finally { setBusy(false) }
  }
  return <div className="space-y-6">
    <div className="flex items-center gap-3 text-primary"><Server className="h-5 w-5" /><span className="text-xs font-medium tracking-[0.16em]">QUNICA SERVER</span></div>
    <div><h1 className="font-serif text-3xl">{zh ? '连接你的服务器' : 'Connect to your server'}</h1>
      <p className="mt-3 text-sm leading-6 text-muted-foreground">{zh ? '工作和结果留在服务器。使用同一账号，在每台设备上继续。' : 'Your work and results stay on the server. Sign in with the same account on every device.'}</p></div>
    <form className="space-y-3" onSubmit={event => { event.preventDefault(); void connect() }}>
      <label htmlFor="qunica-server-origin" className="block text-sm font-medium">{zh ? '服务器地址' : 'Server address'}</label>
      <Input id="qunica-server-origin" type="url" value={address} onChange={event => setAddress(event.target.value)} placeholder="https://qunica.example.com" autoCapitalize="none" autoCorrect="off" spellCheck={false} required disabled={busy} className="h-12 font-mono text-sm" />
      <p className="text-xs leading-5 text-muted-foreground">{zh ? 'HTTPS 域名或受信任的 IP 地址，无需桌面在线。' : 'An HTTPS domain or trusted IP address. Your desktop can stay offline.'}</p>
      {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
      <Button type="submit" className="h-12 w-full justify-between" disabled={busy || !address.trim()}>{zh ? '连接并登录' : 'Connect and sign in'}{busy ? <Loader2 className="h-4 w-4 animate-spin" /> : <ArrowRight className="h-4 w-4" />}</Button>
    </form>
    {onLocal && <Button className="h-12 w-full gap-2" variant="outline" disabled={busy} onClick={() => void connect(true)}><Monitor className="h-4 w-4" />{zh ? '使用本地实例' : 'Use local instance'}</Button>}
  </div>
}
