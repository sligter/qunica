import { useCallback, useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { QRCodeSVG } from 'qrcode.react'
import { useTranslation } from 'react-i18next'
import { Smartphone } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { SettingsSection } from '@/components/ui/settings-row'

interface Status { endpoint: string | null; interfaces: { name: string; address: string }[]; devices: { id: string; name: string; created: number }[] }
interface Offer { uri: string; expiresAt: number }

export function MobileConnection() {
  const { i18n } = useTranslation()
  const zh = i18n.language.startsWith('zh')
  const [status, setStatus] = useState<Status | null>(null)
  const [address, setAddress] = useState('')
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
  return <SettingsSection title={zh ? '手机连接' : 'Phone connection'} description={zh ? '同一 Wi-Fi，扫码加密连接。无需证书或额外 VPN。' : 'Scan to connect over the same Wi-Fi. Encrypted, without certificates or another VPN.'}>
    <div className="space-y-5 py-4">
      <div className="flex flex-wrap items-center gap-3">
        <Smartphone className="h-5 w-5 text-primary" />
        <span className="text-sm">{status?.endpoint ? `${zh ? '已开启' : 'Sharing'} · ${status.endpoint}` : zh ? '手机连接已关闭' : 'Phone connection is off'}</span>
        {status?.endpoint ? <Button variant="outline" disabled={busy} onClick={() => void action(async () => { await invoke('mobile_link_stop'); setOffer(null) })}>{zh ? '关闭连接' : 'Stop sharing'}</Button> : <>
          <select aria-label={zh ? '局域网网卡' : 'LAN interface'} value={address} onChange={e => setAddress(e.target.value)} className="h-11 max-w-full rounded-md border border-input bg-background px-3 text-sm" disabled={busy}>
            {!status?.interfaces.length ? <option value="">{zh ? '未找到局域网网卡' : 'No LAN interface found'}</option> : null}
            {status?.interfaces.map(i => <option key={i.address} value={i.address}>{i.name} · {i.address}</option>)}
          </select>
          <Button disabled={busy || !address} onClick={() => void action(async () => { await invoke('mobile_link_start', { address }); setOffer(await invoke<Offer>('mobile_link_offer')); setNow(Date.now()) })}>{zh ? '开启并生成二维码' : 'Enable and pair'}</Button>
        </>}
      </div>
      {status?.endpoint ? <div className="flex flex-wrap items-start gap-6 rounded-xl border border-border bg-muted/30 p-5">
        {offer && remaining > 0 ? <div className="rounded-lg bg-white p-4"><QRCodeSVG value={offer.uri} size={224} level="M" title={zh ? '手机配对二维码' : 'Phone pairing code'} /></div> : <div className="flex h-56 w-56 items-center justify-center rounded-lg border border-dashed border-border px-6 text-center text-sm text-muted-foreground">{zh ? '生成新的配对二维码' : 'Generate a fresh pairing code'}</div>}
        <div className="min-w-48 flex-1 space-y-3 text-sm">
          <p className="font-medium">{zh ? '在 Android 应用中选择「扫描桌面二维码」' : 'Choose “Scan desktop QR code” in the Android app'}</p>
          <p className="text-muted-foreground">{offer && remaining > 0 ? (zh ? `约 ${remaining} 秒后过期，扫码后失效。` : `Expires in about ${remaining}s. Works once.`) : (zh ? '二维码已过期或尚未生成。' : 'Code expired or has not been generated.')}</p>
          <Button variant="outline" disabled={busy} onClick={() => void action(async () => { setOffer(await invoke<Offer>('mobile_link_offer')); setNow(Date.now()) })}>{zh ? '重新生成' : 'Regenerate'}</Button>
          {offer && remaining > 0 ? <details><summary className="cursor-pointer py-2 text-muted-foreground">{zh ? '手动复制配对链接' : 'Copy pairing link manually'}</summary><textarea aria-label={zh ? '配对链接' : 'Pairing link'} readOnly value={offer.uri} onFocus={e => e.currentTarget.select()} className="h-24 w-full rounded-md border bg-background p-2 font-mono text-xs" /></details> : null}
          <p className="text-xs leading-5 text-muted-foreground">{zh ? '代理软件需绕过局域网。Windows 提示时允许专用网络访问；网络隔离会阻止手机连接。' : 'Bypass LAN addresses in your proxy. Allow private-network access if Windows prompts. Wi-Fi client isolation prevents connection.'}</p>
        </div>
      </div> : null}
      {status?.devices.length ? <div className="divide-y divide-border">{status.devices.map(device => <div key={device.id} className="flex items-center justify-between gap-3 py-3 text-sm"><span>{device.name}<span className="ml-2 text-xs text-muted-foreground">{new Date(device.created * 1000).toLocaleString()}</span></span><Button variant="ghost" disabled={busy} onClick={() => void action(async () => { await invoke('mobile_link_revoke', { id: device.id }) })}>{zh ? '撤销设备' : 'Revoke device'}</Button></div>)}</div> : null}
      {error ? <p role="alert" className="text-sm text-destructive">{error}</p> : null}
    </div>
  </SettingsSection>
}
