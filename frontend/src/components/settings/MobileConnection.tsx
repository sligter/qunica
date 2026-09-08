import { useCallback, useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { QRCodeSVG } from 'qrcode.react'
import { useTranslation } from 'react-i18next'
import { Smartphone } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { SettingsSection } from '@/components/ui/settings-row'
import { useAuthStore } from '@/stores/authStore'

interface DeviceInfo { manufacturer: string; model: string; systemVersion: string; sdkVersion: number; appVersion: string }
interface Status { endpoint: string | null; listen_endpoint?: string | null; interfaces: { name: string; address: string }[]; devices: { id: string; name: string; created: number; deviceInfo?: DeviceInfo }[] }
interface Offer { uri: string; expiresAt: number }

export function MobileConnection() {
  const { i18n } = useTranslation()
  const zh = i18n.language.startsWith('zh')
  const token = useAuthStore(s => s.token)
  const user = useAuthStore(s => s.user)
  const [status, setStatus] = useState<Status | null>(null)
  const [address, setAddress] = useState('')
  const [relay, setRelay] = useState(false)
  const [relayEndpoint, setRelayEndpoint] = useState(() => localStorage.getItem('qunica:relay-endpoint') ?? '')
  const [offer, setOffer] = useState<Offer | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [now, setNow] = useState(Date.now())
  const refresh = useCallback(async () => {
    const next = await invoke<Status>('mobile_link_status')
    setStatus(next)
    setAddress(previous => next.interfaces.some(i => i.address === previous) ? previous : next.interfaces[0]?.address ?? '')
  }, [])
  useEffect(() => {
    void refresh().catch(cause => setError(String(cause)))
    const timer = window.setInterval(() => { setNow(Date.now()); void refresh().catch(() => undefined) }, 3000)
    return () => window.clearInterval(timer)
  }, [refresh])
  async function action(operation: () => Promise<void>) {
    setBusy(true); setError(null)
    try { await operation(); await refresh() }
    catch (cause) { setError(String(cause)) }
    finally { setBusy(false) }
  }
  const remaining = offer ? Math.max(0, Math.ceil(offer.expiresAt - now / 1000)) : 0
  async function generateOffer() {
    const accountToken = useAuthStore.getState().token
    if (!accountToken) throw new Error(zh ? '请先登录桌面账号' : 'Sign in on the desktop first')
    const next = await invoke<Offer>('mobile_link_offer', { accountToken })
    if (useAuthStore.getState().token !== accountToken) return
    setOffer(next); setNow(Date.now())
  }
  useEffect(() => { setOffer(null) }, [token])
  return <SettingsSection title={zh ? '手机连接' : 'Phone connection'} description={zh ? '通过局域网或自己的 VPS 扫码加密连接，无需证书或额外 VPN。' : 'Encrypted pairing over LAN or your own VPS, without certificates or another VPN.'}>
    <div className="space-y-5 py-4">
      {!status?.endpoint ? <div className="space-y-3">
        <div className="flex flex-wrap gap-2" role="group" aria-label={zh ? '连接方式' : 'Connection mode'}>
          <Button variant={relay ? 'outline' : 'default'} aria-pressed={!relay} disabled={busy} onClick={() => setRelay(false)}>{zh ? '同一局域网' : 'Local network'}</Button>
          <Button variant={relay ? 'default' : 'outline'} aria-pressed={relay} disabled={busy} onClick={() => setRelay(true)}>{zh ? 'VPS 中继' : 'VPS relay'}</Button>
        </div>
        {relay ? <div className="max-w-lg space-y-2 rounded-xl border border-border bg-muted/30 p-4">
          <label htmlFor="mobile-relay-endpoint" className="text-sm font-medium">{zh ? '手机访问地址' : 'Phone destination'}</label>
          <Input id="mobile-relay-endpoint" value={relayEndpoint} onChange={e => setRelayEndpoint(e.target.value)} placeholder="relay.example.com:18766" autoCapitalize="none" autoCorrect="off" spellCheck={false} disabled={busy} className="h-11" />
          <p className="text-xs leading-5 text-muted-foreground">{zh ? '先在 VPS 和电脑配置 frp TCP 转发，目标为本机 127.0.0.1:8766。这里只开启加密入口并生成二维码，不会自动部署或确认 VPS 可达。' : 'Set up frp TCP forwarding on your VPS and computer to 127.0.0.1:8766 first. This enables the encrypted listener and creates a QR; it does not deploy or verify your VPS.'}</p>
        </div> : null}
      </div> : null}
      <div className="flex flex-wrap items-center gap-3">
        <Smartphone className="h-5 w-5 text-primary" />
        <span className="text-sm">{status?.endpoint ? `${zh ? '已开启' : 'Sharing'} · ${status.endpoint}` : zh ? '手机连接已关闭' : 'Phone connection is off'}</span>
        {status?.endpoint ? <Button variant="outline" disabled={busy} onClick={() => void action(async () => { await invoke('mobile_link_stop'); setOffer(null) })}>{zh ? '关闭连接' : 'Stop sharing'}</Button> : <>
          {!relay ? <select aria-label={zh ? '局域网网卡' : 'LAN interface'} value={address} onChange={e => setAddress(e.target.value)} className="h-11 max-w-full rounded-md border border-input bg-background px-3 text-sm" disabled={busy}>
            {!status?.interfaces.length ? <option value="">{zh ? '未找到局域网网卡' : 'No LAN interface found'}</option> : null}
            {status?.interfaces.map(i => <option key={i.address} value={i.address}>{i.name} · {i.address}</option>)}
          </select> : null}
          <Button disabled={busy || (relay ? !relayEndpoint.trim() : !address)} onClick={() => void action(async () => {
            await invoke('mobile_link_start', relay ? { address: '127.0.0.1', advertisedEndpoint: relayEndpoint.trim() } : { address })
            if (relay) localStorage.setItem('qunica:relay-endpoint', relayEndpoint.trim())
            await generateOffer()
          })}>{zh ? '开启并生成二维码' : 'Enable and pair'}</Button>
        </>}
      </div>
      {status?.listen_endpoint ? <p className="break-all text-xs text-muted-foreground">{zh ? '电脑监听' : 'Desktop listener'}: {status.listen_endpoint}{status.listen_endpoint !== status.endpoint ? (zh ? ' · VPS 可达性需在手机上验证' : ' · Verify VPS reachability on your phone') : ''}</p> : null}
      {status?.endpoint ? <div className="flex flex-wrap items-start gap-6 rounded-xl border border-border bg-muted/30 p-5">
        {offer && remaining > 0 ? <div className="rounded-lg bg-white p-4"><QRCodeSVG value={offer.uri} size={224} level="M" title={zh ? '手机配对二维码' : 'Phone pairing code'} /></div> : <div className="flex h-56 w-56 items-center justify-center rounded-lg border border-dashed border-border px-6 text-center text-sm text-muted-foreground">{zh ? '生成新的配对二维码' : 'Generate a fresh pairing code'}</div>}
        <div className="min-w-48 flex-1 space-y-3 text-sm">
          <p className="font-medium">{zh ? '在 Android 应用中选择「扫描桌面二维码」' : 'Choose “Scan desktop QR code” in the Android app'}</p>
          <p className="text-muted-foreground">{zh ? `扫码后直接登录当前账号${user ? `：${user.name}` : ''}。请仅让自己的手机扫码。` : `Scanning signs in to your current account${user ? `: ${user.name}` : ''}. Only scan with your own phone.`}</p>
          <p className="text-muted-foreground">{offer && remaining > 0 ? (zh ? `约 ${remaining} 秒后过期，扫码后失效。` : `Expires in about ${remaining}s. Works once.`) : (zh ? '二维码已过期或尚未生成。' : 'Code expired or has not been generated.')}</p>
          <Button variant="outline" disabled={busy} onClick={() => void action(generateOffer)}>{zh ? '重新生成' : 'Regenerate'}</Button>
          {offer && remaining > 0 ? <details><summary className="cursor-pointer py-2 text-muted-foreground">{zh ? '手动复制配对链接' : 'Copy pairing link manually'}</summary><textarea aria-label={zh ? '配对链接' : 'Pairing link'} readOnly value={offer.uri} onFocus={e => e.currentTarget.select()} className="h-24 w-full rounded-md border bg-background p-2 font-mono text-xs" /></details> : null}
          <p className="text-xs leading-5 text-muted-foreground">{status?.listen_endpoint?.startsWith('127.0.0.1:') ? (zh ? '中继连接需新版 APK。电脑及 frpc 必须保持运行；切换连接方式请先关闭连接。VPS 仅转发加密数据。' : 'Relay pairing requires the updated APK. Keep desktop and frpc running; stop sharing before switching modes. The VPS forwards encrypted data only.') : (zh ? '代理软件需绕过局域网。Windows 提示时允许专用网络访问；网络隔离会阻止手机连接。' : 'Bypass LAN addresses in your proxy. Allow private-network access if Windows prompts. Wi-Fi client isolation prevents connection.')}</p>
        </div>
      </div> : null}
      {status?.devices.length ? <div className="divide-y divide-border">{status.devices.map(device => <div key={device.id} className="flex items-center justify-between gap-3 py-3 text-sm">
        <div className="min-w-0 space-y-1 break-words">
          <p className="font-medium">{device.name}</p>
          {device.deviceInfo ? <p className="text-xs text-muted-foreground">{device.deviceInfo.manufacturer} {device.deviceInfo.model} · Android {device.deviceInfo.systemVersion} · API {device.deviceInfo.sdkVersion} · Qunica {device.deviceInfo.appVersion}</p> : <p className="text-xs text-muted-foreground">{zh ? '旧配对未上报型号，更新手机应用后重新扫码可补充。' : 'Update the phone app and pair again to add device details.'}</p>}
          <p className="text-xs text-muted-foreground">{new Date(device.created * 1000).toLocaleString()}</p>
        </div>
        <Button variant="ghost" className="shrink-0" disabled={busy} onClick={() => void action(async () => { await invoke('mobile_link_revoke', { id: device.id }) })}>{zh ? '撤销设备' : 'Revoke device'}</Button>
      </div>)}</div> : null}
      {error ? <p role="alert" className="text-sm text-destructive">{error}</p> : null}
    </div>
  </SettingsSection>
}
