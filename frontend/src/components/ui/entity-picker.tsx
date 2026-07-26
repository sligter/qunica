import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from 'react'
import { Search, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'

/** Row height in px. Drives the scroller cap so it lands on a whole row. */
const ROW_HEIGHT = 32

/** Rows visible before the list scrolls. */
const VISIBLE_ROWS = 8

/**
 * At or below this many items the picker renders as a bare checkbox grid — no
 * search field, no chip tray, no scroller.
 *
 * A search box in front of three skills is chrome the user has to read past to
 * reach a list they could already see whole. Above it, a flat list is the thing
 * that stops working, so the chrome starts earning its place.
 */
const COMPACT_THRESHOLD = 8

/** One selectable row. */
export interface PickerItem {
  id: string
  /** Primary label. Owns the name column. */
  label: string
  /**
   * One-line secondary text filling the rest of the row. Truncates, never wraps.
   * It gets the full remaining width rather than the narrow stub a card allows.
   */
  meta?: string
  /** Renders the label in the mono face — slugs, tool names, ids. */
  monoLabel?: boolean
  /** Matched by search but never rendered (slug, tags, namespaced name). */
  keywords?: string
  /** Group key. Headers render only when more than one group is present. */
  group?: string
  /**
   * Marks the row unselectable and explains why, wired to the checkbox through
   * `aria-describedby` rather than shown as another stacked line.
   */
  disabledReason?: string
  /**
   * Flush-right slot, rendered as a sibling of the row label rather than inside
   * it, so it can hold a real control without nesting one inside a label.
   */
  trailing?: ReactNode
}

export interface EntityPickerProps {
  items: PickerItem[]
  /** Controlled: selection lives in the parent form. */
  selectedIds: string[]
  onChange: (nextIds: string[]) => void
  /** Accessible name for the group and its search field. */
  label: string
  searchPlaceholder?: string
  /** Rendered instead of the list when there is nothing to pick from. */
  empty?: ReactNode
  /** Explicit group ordering; unlisted groups sort after, alphabetically. */
  groupOrder?: string[]
  groupLabel?: (group: string) => string
  /** Noun for the count line, already pluralized by the caller. */
  countLabel?: (total: number, selected: number) => string
  className?: string
}

/**
 * One multi-select list that stays usable from three items to several hundred.
 *
 * Rows are a fixed height with an aligned name column, so a long list scans as a
 * single vertical edge rather than a ragged mosaic; the list caps at
 * {@link VISIBLE_ROWS} and scrolls, so the surrounding form is the same height
 * whether the user owns nine skills or nine hundred.
 *
 * Selection never reorders the list. Re-sorting under the cursor turns
 * multi-select into a game of chase; what is selected stays visible through the
 * chip tray and the selected-only filter instead.
 */
export function EntityPicker({
  items,
  selectedIds,
  onChange,
  label,
  searchPlaceholder,
  empty,
  groupOrder,
  groupLabel,
  countLabel,
  className,
}: EntityPickerProps) {
  const { t } = useTranslation('common')
  const [query, setQuery] = useState('')
  const [selectedOnly, setSelectedOnly] = useState(false)
  const [activeIndex, setActiveIndex] = useState(0)
  const listRef = useRef<HTMLDivElement | null>(null)
  const searchId = useId()

  const selected = useMemo(() => new Set(selectedIds), [selectedIds])
  const compact = items.length <= COMPACT_THRESHOLD

  const sorted = useMemo(() => {
    const byGroup = (item: PickerItem) => {
      const index = groupOrder?.indexOf(item.group ?? '') ?? -1
      return index === -1 ? Number.MAX_SAFE_INTEGER : index
    }
    return [...items].sort(
      (a, b) =>
        byGroup(a) - byGroup(b) ||
        (a.group ?? '').localeCompare(b.group ?? '') ||
        a.label.localeCompare(b.label),
    )
  }, [items, groupOrder])

  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase()
    return sorted.filter((item) => {
      if (selectedOnly && !selected.has(item.id)) return false
      if (!needle) return true
      return (
        item.label.toLowerCase().includes(needle) ||
        item.meta?.toLowerCase().includes(needle) ||
        item.keywords?.toLowerCase().includes(needle)
      )
    })
  }, [sorted, query, selectedOnly, selected])

  // A filter that shortens the list can leave the cursor past its end.
  useEffect(() => {
    setActiveIndex((index) => Math.min(index, Math.max(visible.length - 1, 0)))
  }, [visible.length])

  const toggle = (item: PickerItem) => {
    if (item.disabledReason) return
    onChange(
      selected.has(item.id)
        ? selectedIds.filter((id) => id !== item.id)
        : [...selectedIds, item.id],
    )
  }

  const moveActive = (delta: number) => {
    setActiveIndex((index) => {
      const next = Math.max(0, Math.min(visible.length - 1, index + delta))
      const rows = listRef.current?.querySelectorAll('[data-picker-row]')
      rows?.[next]?.scrollIntoView({ block: 'nearest' })
      return next
    })
  }

  const onListKeyDown = (event: React.KeyboardEvent) => {
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault()
        moveActive(1)
        break
      case 'ArrowUp':
        event.preventDefault()
        moveActive(-1)
        break
      case 'Home':
        event.preventDefault()
        moveActive(-visible.length)
        break
      case 'End':
        event.preventDefault()
        moveActive(visible.length)
        break
      default:
        break
    }
  }

  if (items.length === 0) {
    return <>{empty}</>
  }

  // Below the threshold the whole list already fits, so it renders as a plain
  // grid with none of the chrome a long list needs.
  if (compact) {
    return (
      <div
        role="group"
        aria-label={label}
        className={cn('grid gap-1 sm:grid-cols-2', className)}
      >
        {sorted.map((item) => (
          <PickerRow
            key={item.id}
            item={item}
            checked={selected.has(item.id)}
            onToggle={() => toggle(item)}
          />
        ))}
      </div>
    )
  }

  const groups = groupedRows(visible)
  const showGroupHeadings = groups.length > 1

  return (
    <div className={cn('space-y-2', className)} role="group" aria-label={label}>
      <div className="flex flex-wrap items-center gap-2">
        <div className="relative min-w-0 flex-1">
          <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            id={searchId}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'ArrowDown') {
                event.preventDefault()
                listRef.current?.focus()
              }
            }}
            placeholder={searchPlaceholder ?? t('picker.search')}
            aria-label={searchPlaceholder ?? t('picker.search')}
            className="h-8 pl-8 text-xs"
          />
        </div>
        {selectedIds.length > 0 ? (
          <>
            {/* Filtering to the selection is how a chosen item stays reachable
                at 300 rows without scrolling the whole list to find it. */}
            <Button
              type="button"
              variant={selectedOnly ? 'default' : 'outline'}
              size="sm"
              aria-pressed={selectedOnly}
              onClick={() => setSelectedOnly((on) => !on)}
            >
              {t('picker.selectedCount', { count: selectedIds.length })}
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => {
                onChange([])
                setSelectedOnly(false)
              }}
            >
              {t('picker.clear')}
            </Button>
          </>
        ) : null}
      </div>

      {selectedIds.length > 0 && !selectedOnly ? (
        <ul className="flex max-h-14 flex-wrap gap-1 overflow-y-auto">
          {sorted
            .filter((item) => selected.has(item.id))
            .map((item) => (
              <li key={item.id}>
                <Badge variant="secondary" className="gap-1 py-1 font-normal">
                  <span className="max-w-40 truncate">{item.label}</span>
                  <button
                    type="button"
                    onClick={() => toggle(item)}
                    aria-label={t('picker.remove', { name: item.label })}
                    className="rounded-sm opacity-60 hover:opacity-100"
                  >
                    <X className="h-3 w-3" />
                  </button>
                </Badge>
              </li>
            ))}
        </ul>
      ) : null}

      <div
        ref={listRef}
        tabIndex={0}
        role="listbox"
        aria-multiselectable
        aria-label={label}
        onKeyDown={onListKeyDown}
        style={{ maxHeight: ROW_HEIGHT * VISIBLE_ROWS }}
        className="overflow-y-auto rounded-md border border-border bg-background focus:outline-none focus:ring-1 focus:ring-ring"
      >
        {visible.length === 0 ? (
          <p className="px-3 py-3 text-xs text-muted-foreground">
            {t('state.noMatches')}
          </p>
        ) : (
          groups.map(([group, rows]) => (
            <div key={group}>
              {showGroupHeadings ? (
                <p className="sticky top-0 z-10 bg-background/95 px-2 py-1 text-2xs font-medium uppercase tracking-wider text-muted-foreground backdrop-blur">
                  {groupLabel?.(group) ?? group}
                </p>
              ) : null}
              {rows.map((item) => (
                <PickerRow
                  key={item.id}
                  item={item}
                  checked={selected.has(item.id)}
                  active={visible[activeIndex]?.id === item.id}
                  onToggle={() => toggle(item)}
                />
              ))}
            </div>
          ))
        )}
      </div>

      <div className="flex items-center justify-between text-2xs text-muted-foreground">
        <span>
          {countLabel?.(items.length, selectedIds.length) ??
            // `count` is what selects the plural form; `total` is what the
            // string interpolates.
            t('picker.summary', {
              count: items.length,
              total: items.length,
              selected: selectedIds.length,
            })}
        </span>
        <span className="hidden sm:inline">{t('picker.keyboardHint')}</span>
      </div>
    </div>
  )
}

interface PickerRowProps {
  item: PickerItem
  checked: boolean
  active?: boolean
  onToggle: () => void
}

/**
 * One fixed-height row: checkbox, name column, then the description filling the
 * rest of the width on a single truncating line.
 *
 * A selected row gets a left rail and a tint rather than a solid fill — at
 * thirty rows a column of filled blocks reads as noise, where a rail reads as a
 * column of ticks.
 */
function PickerRow({ item, checked, active, onToggle }: PickerRowProps) {
  const reasonId = useId()
  const disabled = Boolean(item.disabledReason)

  return (
    <div
      data-picker-row
      className={cn(
        'flex items-center gap-2 border-l-2 pl-1.5 pr-2',
        checked ? 'border-primary bg-primary/10' : 'border-transparent',
        active && 'bg-muted',
        disabled && 'opacity-50',
      )}
      style={{ height: ROW_HEIGHT }}
    >
      <label
        className={cn(
          'flex min-w-0 flex-1 items-center gap-2',
          disabled ? 'cursor-not-allowed' : 'cursor-pointer',
        )}
      >
        <input
          type="checkbox"
          className="shrink-0"
          checked={checked}
          disabled={disabled}
          aria-describedby={disabled ? reasonId : undefined}
          onChange={onToggle}
        />
        <span
          className={cn(
            'w-full max-w-64 shrink-0 truncate text-xs',
            item.monoLabel && 'font-mono',
          )}
        >
          {item.label}
        </span>
        <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">
          {item.disabledReason ? (
            <span id={reasonId}>{item.disabledReason}</span>
          ) : (
            item.meta
          )}
        </span>
      </label>
      {item.trailing ? <div className="shrink-0">{item.trailing}</div> : null}
    </div>
  )
}

/** Bucket rows by group, preserving the order they arrive in. */
function groupedRows(items: PickerItem[]): Array<[string, PickerItem[]]> {
  const groups: Array<[string, PickerItem[]]> = []
  for (const item of items) {
    const key = item.group ?? ''
    const bucket = groups.find(([group]) => group === key)
    if (bucket) bucket[1].push(item)
    else groups.push([key, [item]])
  }
  return groups
}
