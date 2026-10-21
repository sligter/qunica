import { useContext, useState } from 'react'
import { Check, ChevronDown, Monitor, Plus, Server } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { desktopConnectionId, useDesktopConnection } from '@/lib/desktopConnection'
import { DesktopInstanceContext } from '@/lib/desktopInstanceContext'

export function DesktopInstanceSwitcher() {
  const actions = useContext(DesktopInstanceContext)
  const { connection, instances } = useDesktopConnection()
  const { t } = useTranslation('navigation')
  const [open, setOpen] = useState(false)
  if (!actions) return null
  const current = desktopConnectionId(connection)
  const label = connection.mode === 'local' ? t('instances.local') : new URL(connection.origin).host

  return <Popover open={open} onOpenChange={setOpen}>
    <PopoverTrigger asChild>
      <Button variant="ghost" size="sm" className="h-7 min-w-0 max-w-20 gap-1 px-1.5 text-muted-foreground" aria-label={t('instances.switch')} title={label}>
        <span className="truncate text-xs">{label}</span>
        <ChevronDown className="h-3 w-3 shrink-0" aria-hidden="true" />
      </Button>
    </PopoverTrigger>
    <PopoverContent align="start" className="w-72 max-w-[calc(100vw-2rem)]">
      <p className="px-2 py-1.5 text-xs font-medium text-muted-foreground">{t('instances.switch')}</p>
      <div role="group" aria-label={t('instances.saved')} className="max-h-64 overflow-y-auto">
        {instances.map(instance => {
          const id = desktopConnectionId(instance)
          const selected = id === current
          const Icon = instance.mode === 'local' ? Monitor : Server
          return <button key={id} type="button" aria-pressed={selected} className="flex w-full items-center gap-2 rounded px-2 py-2 text-left text-sm hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" onClick={() => {
            setOpen(false)
            void actions.select(instance)
          }}>
            <Icon className="h-4 w-4 shrink-0 text-muted-foreground" aria-hidden="true" />
            <span className="min-w-0 flex-1 truncate" title={id}>{instance.mode === 'local' ? t('instances.local') : instance.origin}</span>
            {selected ? <Check className="h-4 w-4 shrink-0 text-primary" aria-hidden="true" /> : null}
          </button>
        })}
      </div>
      <div className="mt-1 border-t border-border pt-1">
        <Button variant="ghost" size="sm" className="w-full justify-start" onClick={() => { setOpen(false); actions.add() }}><Plus className="h-4 w-4" aria-hidden="true" />{t('instances.add')}</Button>
      </div>
    </PopoverContent>
  </Popover>
}
