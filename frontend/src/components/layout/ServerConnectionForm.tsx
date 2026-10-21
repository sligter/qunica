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
    <div className="flex items-center gap-3 text-primary">{onLocal ? <Monitor className="h-5 w-5" /> : <Server className="h-5 w-5" />}<span className="text-xs font-medium tracking-[0.16em]">QUNICA {onLocal ? 'DESKTOP' : 'SERVER'}</span></div>
    <div><h1 className="font-serif text-3xl">{onLocal ? (zh ? '选择工作模式' : 'Choose your workspace') : (zh ? '连接你的服务器' : 'Connect to your server')}</h1>
      <p className="mt-3 text-sm leading-6 text-muted-foreground">{onLocal ? (zh ? '直接在这台电脑上工作，或连接已有服务器。记住选择，下次自动进入。' : 'Work on this computer or connect to an existing server. Your choice is remembered for next time.') : (zh ? '工作和结果留在服务器。使用同一账号，在每台设备上继续。' : 'Your work and results stay on the server. Sign in with the same account on every device.')}</p></div>
    {onLocal && <section className="space-y-3 rounded-xl border border-primary/30 bg-primary/5 p-5">
      <div className="flex items-center justify-between gap-3"><h2 className="font-medium">{zh ? 'Local 本地模式' : 'Local mode'}</h2><span className="text-xs text-primary">{zh ? '无需 Docker' : 'No Docker required'}</span></div>
      <p className="text-sm leading-6 text-muted-foreground">{zh ? '自动启动内置服务，使用这台电脑上的文件、Agent 和历史记录。模型调用仍需相应的网络与账号配置。' : 'Starts the built-in service with your files, agents and history on this computer. Model calls still require their network and account settings.'}</p>
      <Button className="h-12 w-full justify-between" disabled={busy} onClick={() => void connect(true)}>{zh ? '进入本地模式' : 'Enter local mode'}{busy ? <Loader2 className="h-4 w-4 animate-spin" /> : <ArrowRight className="h-4 w-4" />}</Button>
      <p className="text-xs leading-5 text-muted-foreground">{zh ? '只有使用受控电脑时才需要 Docker / Supervisor；已有电脑绑定需在电脑设置中切换。' : 'Docker / Supervisor is only needed for controlled computers. Manage existing execution bindings in computer settings.'}</p>
    </section>}
    <form className="space-y-3" onSubmit={event => { event.preventDefault(); void connect() }}>
      <label htmlFor="qunica-server-origin" className="block text-sm font-medium">{zh ? '服务器地址' : 'Server address'}</label>
      <Input id="qunica-server-origin" type="url" value={address} onChange={event => setAddress(event.target.value)} placeholder="https://qunica.example.com" autoCapitalize="none" autoCorrect="off" spellCheck={false} required disabled={busy} className="h-12 font-mono text-sm" />
      <p className="text-xs leading-5 text-muted-foreground">{zh ? 'HTTPS 域名或受信任的 IP 地址，无需桌面在线。' : 'An HTTPS domain or trusted IP address. Your desktop can stay offline.'}</p>
      {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
      <Button type="submit" variant={onLocal ? 'outline' : 'default'} className="h-12 w-full justify-between" disabled={busy || !address.trim()}>{zh ? '连接并登录' : 'Connect and sign in'}{busy ? <Loader2 className="h-4 w-4 animate-spin" /> : <ArrowRight className="h-4 w-4" />}</Button>
    </form>
  </div>
}
