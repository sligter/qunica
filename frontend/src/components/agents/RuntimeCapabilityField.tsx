import { useEffect, useMemo, useRef, useState } from 'react'
import { Check, ChevronDown, LoaderCircle, RefreshCw, TriangleAlert } from 'lucide-react'
import { useTranslation } from 'react-i18next'

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
  const { t } = useTranslation('agents')
  const listId = `${id}-available-values`
  const statusId = `${id}-capability-status`
  const fieldRef = useRef<HTMLDivElement>(null)
  const lastCommittedValue = useRef(value)
  const pendingLocalValue = useRef<string | null>(null)
  const [open, setOpen] = useState(false)
  const [activeIndex, setActiveIndex] = useState(-1)
  const [filter, setFilter] = useState('')
  const filteredOptions = useMemo(() => {
    const query = filter.trim().toLocaleLowerCase()
    if (!query) return options
    return options.filter((option) =>
      [option.value, option.label, option.description]
        .filter(Boolean)
        .some((text) => text!.toLocaleLowerCase().includes(query)),
    )
  }, [filter, options])

  useEffect(() => {
    if (pendingLocalValue.current === value) {
      pendingLocalValue.current = null
      return
    }
    pendingLocalValue.current = null
    lastCommittedValue.current = value
  }, [value])

  useEffect(() => {
    if (!open) return

    const closeOnOutsidePointerDown = (event: PointerEvent) => {
      if (!fieldRef.current?.contains(event.target as Node)) {
        setOpen(false)
        setActiveIndex(-1)
      }
    }

    document.addEventListener('pointerdown', closeOnOutsidePointerDown)
    return () => document.removeEventListener('pointerdown', closeOnOutsidePointerDown)
  }, [open])

  const commit = (nextValue: string) => {
    if (!onCommit || nextValue === lastCommittedValue.current) return
    lastCommittedValue.current = nextValue
    onCommit(nextValue)
  }

  const selectOption = (option: RuntimeCapabilityOption) => {
    pendingLocalValue.current = option.value
    onChange(option.value)
    commit(option.value)
    setOpen(false)
    setActiveIndex(-1)
    setFilter('')
  }

  const status = isLoading
    ? t('states.loadingValues')
    : stale
      ? t('states.staleValues')
      : warning

  return (
    <div className="space-y-1.5">
      <Label htmlFor={id}>{label}</Label>
      <div className="flex items-start gap-1.5">
        <div ref={fieldRef} className="relative min-w-0 flex-1">
          <Input
            id={id}
            role="combobox"
            value={value}
            placeholder={placeholder}
            aria-autocomplete="list"
            aria-controls={open ? listId : undefined}
            aria-expanded={open}
            aria-activedescendant={
              activeIndex >= 0 ? `${listId}-option-${activeIndex}` : undefined
            }
            aria-describedby={status ? statusId : undefined}
            className="pr-9"
            onFocus={() => {
              setOpen(filteredOptions.length > 0)
              setActiveIndex(-1)
              setFilter('')
            }}
            onChange={(event) => {
              const nextValue = event.target.value
              pendingLocalValue.current = nextValue
              onChange(nextValue)
              setOpen(true)
              setActiveIndex(-1)
              setFilter(nextValue)
              if (options.some((option) => option.value === nextValue)) {
                commit(nextValue)
              }
            }}
            onBlur={() => {
              window.setTimeout(() => setOpen(false), 120)
              commit(value)
            }}
            onKeyDown={(event) => {
              if (event.key === 'ArrowDown') {
                event.preventDefault()
                setOpen(filteredOptions.length > 0)
                setActiveIndex((index) => Math.min(index + 1, filteredOptions.length - 1))
                return
              }
              if (event.key === 'ArrowUp') {
                event.preventDefault()
                setActiveIndex((index) => Math.max(index - 1, 0))
                return
              }
              if (event.key === 'Escape') {
                setOpen(false)
                setActiveIndex(-1)
                return
              }
              if (event.key === 'Enter') {
                event.preventDefault()
                const option = filteredOptions[activeIndex]
                if (option) {
                  selectOption(option)
                } else {
                  commit(event.currentTarget.value)
                }
              }
            }}
          />
          <button
            type="button"
            aria-label={t('actions.showOptions', { label: label.toLocaleLowerCase() })}
            aria-expanded={open}
            className="absolute inset-y-0 right-0 grid w-9 place-items-center text-muted-foreground transition-colors hover:text-foreground"
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => {
              setOpen((visible) => !visible)
              setActiveIndex(-1)
              setFilter('')
            }}
          >
            <ChevronDown className="h-4 w-4" />
          </button>
          {open && filteredOptions.length > 0 && (
            <div
              id={listId}
              role="listbox"
              aria-label={t('actions.valueOptions', { label })}
              className="absolute z-30 mt-1 max-h-64 w-full overflow-y-auto rounded-md border border-border bg-card p-1 shadow-lg"
            >
              {filteredOptions.map((option, index) => {
                const selected = option.value === value
                return (
                  <button
                    key={option.value}
                    id={`${listId}-option-${index}`}
                    type="button"
                    role="option"
                    aria-selected={selected}
                    className={cn(
                      'flex w-full items-start gap-2 rounded-sm px-2 py-1.5 text-left text-sm outline-none transition-colors',
                      selected || index === activeIndex
                        ? 'bg-accent text-accent-foreground'
                        : 'hover:bg-muted',
                    )}
                    onMouseDown={(event) => event.preventDefault()}
                    onClick={() => selectOption(option)}
                    >
                      <span className="min-w-0 flex-1">
                      <span className="block truncate font-medium">{option.label}</span>
                      {(option.description || option.label !== option.value) && (
                        <span className="block truncate text-xs text-muted-foreground">
                          {option.description ?? option.value}
                        </span>
                      )}
                    </span>
                    {selected && <Check className="mt-0.5 h-4 w-4 shrink-0" />}
                  </button>
                )
              })}
            </div>
          )}
        </div>
        {onRefresh ? (
          <Button
            type="button"
            variant="outline"
            size="icon"
            className="shrink-0"
            aria-label={t('actions.refreshValues')}
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
            'flex items-start gap-1 text-2xs leading-4',
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
