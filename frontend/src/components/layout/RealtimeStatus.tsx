import { useTranslation } from 'react-i18next'
import { useAccountSyncStore } from '@/stores/accountSyncStore'
import { Button } from '@/components/ui/button'

export function RealtimeStatus() {
  const { i18n } = useTranslation()
  const zh = i18n.language.startsWith('zh')
  const { status, error } = useAccountSyncStore()
  if (status === 'connected') return null
  return <div role="status" className="fixed right-3 bottom-3 z-50 max-w-sm rounded-lg border border-border bg-card px-3 py-2 text-xs shadow-sm">
    <span className={status === 'error' ? 'text-destructive' : 'text-muted-foreground'}>{status === 'error'
      ? `${zh ? '实时同步不可用' : 'Realtime sync unavailable'}: ${error ?? ''}`
      : zh ? '正在恢复实时同步…' : 'Reconnecting realtime sync…'}</span>
    {status === 'error' && <Button variant="ghost" size="sm" onClick={() => window.location.reload()}>{zh ? '重新连接' : 'Reconnect'}</Button>}
  </div>
}
