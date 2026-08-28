import { X } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'

interface SearchInputProps {
  value: string
  onChange: (value: string) => void
  /** Placeholder + accessible name; callers pass their namespaced label. */
  label: string
  className?: string
}

/**
 * The one search field used on collection surfaces: leading icon, clear
 * button when non-empty. Kept dumb — filtering logic stays with the caller,
 * which owns its own match rules (name only vs. name + path).
 */
export function SearchInput({
  value,
  onChange,
  label,
  className,
}: SearchInputProps) {
  const { t } = useTranslation('common')
  const showClear = value.length > 0

  return (
    <div className={cn('relative w-full sm:max-w-xs', className)}>
      <SearchIcon aria-hidden className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
      <Input
        type="search"
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={label}
        aria-label={label}
        // Webkit shows its own × inside type=search; ours is the single
        // control, so suppress the native one.
        className="h-8 bg-card pl-8 pr-9 text-xs [&::-webkit-search-cancel-button]:hidden"
      />
      {showClear ? (
        <button
          type="button"
          onClick={() => onChange('')}
          aria-label={t('actions.clear')}
          // Sized for a fingertip rather than for the glyph: `p-1.5` around a
          // 14px icon is a 26px target, and the field reserves `pr-9` for it so
          // the button never sits over the text it clears.
          className="absolute right-1 top-1/2 -translate-y-1/2 rounded p-1.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <X className="h-3.5 w-3.5" />
        </button>
      ) : null}
    </div>
  )
}

function SearchIcon({ className }: { className?: string }) {
  return (
    <svg
      aria-hidden
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
    >
      <circle cx="11" cy="11" r="8" />
      <path d="m21 21-4.3-4.3" />
    </svg>
  )
}
