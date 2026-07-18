import { Label } from '@/components/ui/label'
import { useTranslation } from 'react-i18next'
import { thinkingLevelOptions, type ThinkingLevel } from '@/components/agents/thinkingLevel'
import { cn } from '@/lib/utils'

interface ThinkingLevelControlProps {
  value: ThinkingLevel
  onChange: (value: ThinkingLevel) => void
}

export function ThinkingLevelControl({ value, onChange }: ThinkingLevelControlProps) {
  const { t } = useTranslation('agents')
  const selected = thinkingLevelOptions.find((option) => option.value === value)
  const optionLabel = (level: ThinkingLevel) => t(`thinking.${level}`)

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <Label>{t('fields.thinkingLevel')}</Label>
        <span className="text-xs text-muted-foreground">{selected ? optionLabel(selected.value) : null}</span>
      </div>
      <div
        className="grid grid-cols-5 gap-1 rounded-md border border-border bg-muted/40 p-1"
        aria-label={t('fields.thinkingLevel')}
      >
        {thinkingLevelOptions.map((option) => {
          const active = option.value === value
          return (
            <button
              key={option.value}
              type="button"
              aria-pressed={active}
              onClick={() => onChange(option.value)}
              className={cn(
                'h-8 rounded-sm px-1 text-xs font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
                active
                  ? 'bg-background text-foreground shadow-sm'
                  : 'text-muted-foreground hover:bg-background/70 hover:text-foreground',
              )}
            >
              {optionLabel(option.value)}
            </button>
          )
        })}
      </div>
    </div>
  )
}
