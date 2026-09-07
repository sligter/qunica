import { useEffect, useState, type ReactNode } from 'react'
import { Cable, ChevronLeft, Loader2, QrCode, ShieldCheck } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { androidDesktopAddress, changeAndroidDesktopEndpoint, hasAndroidDesktopPairing, initializeAndroidSession, pairAndroidDesktop, retryAndroidPersistence, useAndroidSession } from '@/lib/androidSession'
import { useAuthStore } from '@/stores/authStore'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

export function AndroidShell({ children }: { children: ReactNode }) {
  const { i18n } = useTranslation()
  const zh = i18n.language.startsWith('zh')
  const { server, ready, error } = useAndroidSession()
  const token = useAuthStore(s => s.token)
  const [editing, setEditing] = useState(false)
  const [address, setAddress] = useState('')
  const [destination, setDestination] = useState('')
  const [busy, setBusy] = useState(false)
  const [connectionError, setConnectionError] = useState<string | null>(null)
  const initialize = () => {
    void initializeAndroidSession().then(token => useAuthStore.setState({ token })).catch(() => undefined)
  }
  useEffect(initialize, [])

  async function connect(offer: string) {
    setBusy(true); setConnectionError(null)
    try {
      await pairAndroidDesktop(offer)
      useAuthStore.getState().logout()
      window.history.replaceState(null, '', '/')
      // Rebuild the router and all query scopes against the newly saved origin.
      window.location.reload()
    } catch (cause) {
      setConnectionError(zh ? `连接失败：${String(cause)}。请检查桌面手机连接及局域网或 VPS 转发；配对码过期请重新生成。` : `Connection failed: ${String(cause)}. Check desktop sharing and LAN or VPS forwarding. Generate a fresh code if it expired.`)
    } finally { setBusy(false) }
  }

  async function scanDesktop() {
    setBusy(true); setConnectionError(null)
    try {
      const { scan, Format, requestPermissions } = await import('@tauri-apps/plugin-barcode-scanner')
      const permission = await requestPermissions()
      if (permission !== 'granted') throw new Error(zh ? '需要相机权限才能扫码，也可粘贴配对链接。' : 'Camera permission is required, or paste the pairing link.')
      const result = await scan({ formats: [Format.QRCode], windowed: false })
      await connect(result.content)
    } catch (cause) { setConnectionError(String(cause)) }
    finally { setBusy(false) }
  }

  if (!ready || !server || editing) return (
    <main className="app-safe-area flex h-full min-h-0 flex-col overflow-y-auto bg-background px-6 py-8">
      <header className="mb-12 flex items-center justify-between">
        <span className="font-serif text-2xl font-semibold">Qunica<span className="ml-2 text-xs font-sans font-normal text-muted-foreground">Android</span></span>
        {server && ready ? <Button variant="ghost" size="icon" aria-label={zh ? '返回' : 'Back'} onClick={() => setEditing(false)}><ChevronLeft /></Button> : null}
      </header>
      <div className="mx-auto w-full max-w-md flex-1">
        <div className="mb-6 flex h-14 w-14 items-center justify-center rounded-2xl border border-border bg-muted text-primary"><Cable size={26} /></div>
        <p className="mb-3 text-xs tracking-[0.2em] text-primary">YOUR DESKTOP, WITH YOU</p>
        <h1 className="mb-3 font-serif text-3xl leading-tight">{zh ? '连接你的工作台' : 'Connect your workspace'}</h1>
        <p className="mb-8 text-sm leading-6 text-muted-foreground">{zh ? '在桌面「设置 → 手机连接」选择局域网或 VPS 中继，生成二维码后扫码。外出使用需先配置 VPS 转发，无需证书。' : 'Choose LAN or VPS relay in desktop Settings → Phone connection and scan its QR. Remote access requires VPS forwarding, without certificates.'}</p>
        {!ready ? (
          <div role="status" className="space-y-3 text-sm">
            {error ? <><p>{zh ? '无法读取安全存储，请重试。' : 'Unable to read secure storage. Try again.'}</p><Button onClick={initialize}>{zh ? '重试' : 'Retry'}</Button></> : <Loader2 className="animate-spin" />}
          </div>
        ) : (
          <form onSubmit={event => { event.preventDefault(); void connect(address) }} className="space-y-4">
            <Button type="button" disabled={busy} onClick={() => void scanDesktop()} className="h-12 w-full gap-2">{busy ? <Loader2 className="h-4 w-4 animate-spin" /> : <QrCode className="h-4 w-4" />}{zh ? '扫描桌面二维码' : 'Scan desktop QR code'}</Button>
            <label htmlFor="android-server" className="block pt-4 text-sm font-medium">{zh ? '或粘贴配对链接' : 'Or paste a pairing link'}</label>
            <Input id="android-server" type="text" autoCapitalize="none" autoCorrect="off" autoComplete="off" spellCheck={false} placeholder="qunica://pair?data=…" value={address} onChange={event => setAddress(event.target.value)} required disabled={busy} className="h-12 text-base" />
            <p className="text-xs leading-5 text-muted-foreground">{zh ? '配对链接两分钟有效，只能使用一次。配对后使用工作台账户登录。' : 'Pairing links expire in two minutes and work once. Sign in with your workspace account after pairing.'}</p>
            {connectionError ? <p role="alert" className="text-sm text-destructive">{connectionError}</p> : null}
            <Button type="submit" variant="outline" disabled={busy || !address.trim()} className="h-12 w-full">{zh ? '使用链接连接' : 'Connect using link'}</Button>
          </form>
        )}
        {ready && hasAndroidDesktopPairing() ? <form className="mt-6 space-y-3 rounded-xl border border-border p-4" onSubmit={event => {
          event.preventDefault(); setBusy(true); setConnectionError(null)
          void changeAndroidDesktopEndpoint(destination).then(() => window.location.reload())
            .catch(cause => setConnectionError(String(cause))).finally(() => setBusy(false))
        }}>
          <label htmlFor="android-destination" className="text-sm font-medium">{zh ? '更换已配对电脑的连接地址' : 'Change paired desktop address'}</label>
          <Input id="android-destination" value={destination} onChange={event => setDestination(event.target.value)} placeholder={androidDesktopAddress() ?? 'relay.example.com:18766'} autoCapitalize="none" autoCorrect="off" spellCheck={false} disabled={busy} className="h-12" />
          <p className="text-xs leading-5 text-muted-foreground">{zh ? '保留配对身份，验证仍是原电脑后保存。不需要重新扫码。' : 'Keeps your pairing and saves only after verifying the same desktop. No new QR needed.'}</p>
          <Button type="submit" variant="outline" disabled={busy || !destination.trim()} className="min-h-11 w-full">{zh ? '验证并保存地址' : 'Verify and save address'}</Button>
        </form> : null}
      </div>
      <p className="mx-auto mt-10 flex max-w-md items-center gap-2 text-xs text-muted-foreground"><ShieldCheck className="h-4 w-4 shrink-0" />{zh ? '登录凭据使用 Android Keystore 加密保存' : 'Credentials are encrypted using Android Keystore'}</p>
    </main>
  )

  return <>
    {!token ? <div className="app-safe-area flex shrink-0 items-center justify-between gap-2 border-b border-border bg-background px-3 py-1 text-xs"><span className="truncate text-muted-foreground">{androidDesktopAddress()}</span><Button variant="ghost" size="sm" onClick={() => { setAddress(''); setEditing(true) }}>{zh ? '重新配对' : 'Pair another desktop'}</Button></div> : null}
    {error ? <div role="alert" className="shrink-0 bg-destructive/10 px-3 py-2 text-sm">{zh ? '安全存储写入失败，请重试后再关闭应用。' : 'Secure storage failed. Retry before closing the app.'}<Button variant="ghost" onClick={() => { void retryAndroidPersistence().catch(() => undefined) }}>{zh ? '重试' : 'Retry'}</Button></div> : null}
    {children}
  </>
}
