import { useTranslation } from 'react-i18next'
import { Label } from '@/components/ui/label'
import { useEngineCapabilities } from '@/hooks/useEngineRollout'

export function EngineChoiceField({ id, value, onChange }: {
  id: string
  value: 'native' | 'rig'
  onChange: (value: 'native' | 'rig') => void
}) {
  const { t } = useTranslation('agents')
  const capabilities = useEngineCapabilities()
  const available = capabilities.data?.rig_available === true
  return <div className="space-y-1.5">
    <Label htmlFor={id}>{t('engine.label')}</Label>
    <select id={id} value={value} onChange={event => onChange(event.target.value as 'native' | 'rig')}
      className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm">
      <option value="native">{t('engine.native')}</option>
      <option value="rig" disabled={!available}>{t('engine.rig')}</option>
    </select>
    <p className="text-xs text-muted-foreground">{t('engine.description')}</p>
    {!available && <p className="text-xs text-muted-foreground">{t('engine.unavailable')}</p>}
    {capabilities.data?.rig_paused && <p className="text-xs text-warning-foreground">{t('engine.paused')}</p>}
  </div>
}
