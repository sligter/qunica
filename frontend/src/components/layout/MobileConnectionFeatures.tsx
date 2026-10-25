import { CalendarClock, Check, Monitor, PanelsTopLeft } from 'lucide-react'
import { useTranslation } from 'react-i18next'

/** Keep connection choices explicit: the paired transport cannot carry desktop RFB. */
export function MobileConnectionFeatures({ paired = false }: { paired?: boolean }) {
  const { i18n } = useTranslation()
  const zh = i18n.language.startsWith('zh')
  return <div className="rounded-xl border border-border bg-muted/30 p-4 text-xs">
    <p className="mb-3 font-medium">{paired ? (zh ? '桌面配对支持' : 'With desktop pairing') : (zh ? '服务器直连支持' : 'With a direct server connection')}</p>
    <ul className="space-y-3 text-muted-foreground">
      <li className="flex items-center gap-2"><PanelsTopLeft className="h-4 w-4 shrink-0" aria-hidden />{zh ? '聊天、审批与独立任务并行' : 'Chat, approvals and parallel tasks'}<Check className="ml-auto h-3.5 w-3.5 shrink-0 text-primary" aria-hidden /></li>
      <li className="flex items-center gap-2"><CalendarClock className="h-4 w-4 shrink-0" aria-hidden />{zh ? '定时任务、文件与交付物' : 'Scheduled tasks, files and artifacts'}<Check className="ml-auto h-3.5 w-3.5 shrink-0 text-primary" aria-hidden /></li>
      <li className="flex items-start gap-2"><Monitor className="h-4 w-4 shrink-0" aria-hidden /><span>{paired ? (zh ? '观察或接管电脑桌面，请使用服务器直连。' : 'Use a direct server connection to view or control a computer desktop.') : (zh ? '观察与接管已配置的电脑桌面' : 'View and control configured computer desktops')}</span>{!paired && <Check className="ml-auto h-3.5 w-3.5 shrink-0 text-primary" aria-hidden />}</li>
    </ul>
  </div>
}
