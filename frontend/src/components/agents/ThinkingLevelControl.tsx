import { Label } from '@/components/ui/label'
import { thinkingLevelOptions, type ThinkingLevel } from '@/components/agents/thinkingLevel'
import { cn } from '@/lib/utils'

interface ThinkingLevelControlProps {
  value: ThinkingLevel
  onChange: (value: ThinkingLevel) => void
}

export function ThinkingLevelControl({ value, onChange }: ThinkingLevelControlProps) {
  const selected = thinkingLevelOptions.find((option) => option.value === value)

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <Label>Thinking level</Label>
        <span className="text-xs text-muted-foreground">{selected?.label}</span>
      </div>
      <div
        className="grid grid-cols-5 gap-1 rounded-md border border-border bg-muted/40 p-1"
        aria-label="Thinking level"
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
              {option.label}
            </button>
          )
        })}
      </div>
    </div>
  )
}
