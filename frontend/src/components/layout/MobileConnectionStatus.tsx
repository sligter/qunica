import { useContext } from 'react'
import { Cable, Server, ArrowLeftRight } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { AndroidConnectionContext } from './mobileChrome'

export function MobileConnectionStatus() {
  const connection = useContext(AndroidConnectionContext)
  const { i18n } = useTranslation()
  if (!connection) return null
  const zh = i18n.language.startsWith('zh')
  const Icon = connection.paired ? Cable : Server
  return <section className="mx-3 mb-3 shrink-0 rounded-xl border border-border bg-background p-3">
    <div className="flex items-center gap-2 text-xs font-medium text-primary"><Icon className="h-3.5 w-3.5" aria-hidden />{connection.paired ? (zh ? '桌面配对' : 'Desktop pairing') : (zh ? '服务器直连' : 'Direct server')}</div>
    <p className="mt-1 break-all text-xs leading-5 text-muted-foreground">{connection.address}</p>
    <Button variant="ghost" size="sm" className="mt-2 w-full justify-start px-0" onClick={connection.edit}><ArrowLeftRight className="h-3.5 w-3.5" aria-hidden />{zh ? '切换连接' : 'Switch connection'}</Button>
  </section>
}
