import { useEffect, useRef } from 'react'
import { LoaderCircle, RefreshCw, TriangleAlert } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { cn } from '@/lib/utils'

export interface RuntimeCapabilityOption {
  value: string
  label: string
  description?: string | null
}

interface RuntimeCapabilityFieldProps {
  id: string
  label: string
  value: string
  options: RuntimeCapabilityOption[]
  placeholder: string
  onChange: (value: string) => void
  onCommit?: (value: string) => void
  onRefresh?: () => void
  isLoading?: boolean
  stale?: boolean
  warning?: string | null
}

export function RuntimeCapabilityField({
  id,
  label,
  value,
  options,
  placeholder,
  onChange,
  onCommit,
  onRefresh,
  isLoading = false,
  stale = false,
  warning = null,
}: RuntimeCapabilityFieldProps) {
  const listId = `${id}-available-values`
  const statusId = `${id}-capability-status`
  const lastCommittedValue = useRef(value)
  const pendingLocalValue = useRef<string | null>(null)

  useEffect(() => {
    if (pendingLocalValue.current === value) {
      pendingLocalValue.current = null
      return
    }
    pendingLocalValue.current = null
    lastCommittedValue.current = value
  }, [value])

  const commit = (nextValue: string) => {
    if (!onCommit || nextValue === lastCommittedValue.current) return
    lastCommittedValue.current = nextValue
    onCommit(nextValue)
  }

  const status = isLoading
    ? 'Loading available values...'
    : stale
      ? 'Runtime settings changed. Refresh available values.'
      : warning

  return (
    <div className="space-y-1.5">
      <Label htmlFor={id}>{label}</Label>
      <div className="flex items-center gap-1.5">
        <Input
          id={id}
          list={listId}
          value={value}
          placeholder={placeholder}
          aria-describedby={status ? statusId : undefined}
          onChange={(event) => {
            const nextValue = event.target.value
            pendingLocalValue.current = nextValue
            onChange(nextValue)
            if (options.some((option) => option.value === nextValue)) {
              commit(nextValue)
            }
          }}
          onBlur={() => commit(value)}
          onKeyDown={(event) => {
            if (event.key !== 'Enter') return
            event.preventDefault()
            commit(event.currentTarget.value)
          }}
        />
        <datalist id={listId}>
          {options.map((option) => (
            <option key={option.value} value={option.value} label={option.label}>
              {option.description ?? option.label}
            </option>
          ))}
        </datalist>
        {onRefresh ? (
          <Button
            type="button"
            variant="outline"
            size="icon"
            className="shrink-0"
            aria-label="Refresh available values"
            disabled={isLoading}
            onClick={onRefresh}
          >
            <RefreshCw className={cn('h-4 w-4', isLoading && 'animate-spin')} />
          </Button>
        ) : (
          isLoading && <LoaderCircle className="h-4 w-4 shrink-0 animate-spin text-muted-foreground" />
        )}
      </div>
      {status && (
        <p
          id={statusId}
          role="status"
          className={cn(
            'flex items-start gap-1 text-[11px] leading-4',
            stale || warning ? 'text-warning-foreground' : 'text-muted-foreground',
          )}
        >
          {(stale || warning) && <TriangleAlert className="mt-0.5 h-3 w-3 shrink-0" />}
          <span>{status}</span>
        </p>
      )}
    </div>
  )
}
