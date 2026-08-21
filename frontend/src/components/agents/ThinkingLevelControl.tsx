import { Label } from '@/components/ui/label'
import { useTranslation } from 'react-i18next'
import { thinkingLevelValues, type ThinkingLevel } from '@/components/agents/thinkingLevel'

interface ThinkingLevelControlProps {
  value: ThinkingLevel
  onChange: (value: ThinkingLevel) => void
}

export function ThinkingLevelControl({ value, onChange }: ThinkingLevelControlProps) {
  const { t } = useTranslation('agents')
  const optionLabel = (level: ThinkingLevel) => t(`thinking.${level}`)

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <Label>{t('fields.thinkingLevel')}</Label>
        <span className="text-xs text-muted-foreground">{optionLabel(value)}</span>
      </div>
      <input
        type="range"
        min={0}
        max={thinkingLevelValues.length - 1}
        step={1}
        value={thinkingLevelValues.indexOf(value)}
        onChange={(event) => onChange(thinkingLevelValues[event.currentTarget.valueAsNumber])}
        className="h-2 w-full cursor-pointer accent-primary"
        aria-label={t('fields.thinkingLevel')}
        aria-valuetext={optionLabel(value)}
      />
    </div>
  )
}
