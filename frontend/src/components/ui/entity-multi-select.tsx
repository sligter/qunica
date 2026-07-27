import * as React from 'react'
import { Check, Search, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'

export interface EntityMultiSelectItem {
  id: string
  name: string
  description?: string | null
  /** Extra text folded into the search index (aliases, tags, runtime names). */
  keywords?: string[]
  /** Short tag rendered at the end of the row, e.g. a runtime kind. */
  badge?: string | null
  disabled?: boolean
  disabledReason?: string | null
}

export interface EntityMultiSelectProps {
  /** Full candidate set. Filtering, ordering, and windowing are owned here. */
  items: EntityMultiSelectItem[]
  selectedIds: string[]
  onChange: (next: string[]) => void
  /** Accessible name for the option list, e.g. "Agent as tool". */
  label: string
  searchPlaceholder: string
  /** Rendered when there is nothing to pick from at all. */
  emptyText: string
  /** Prefix rendered before every name, e.g. "@". */
  namePrefix?: string
  /** At or below this many items, search and the footer stay hidden. */
  searchThreshold?: number
  id?: string
  className?: string
}

/** Rows rendered before the list asks for more; keeps the DOM small at 100+ items. */
const PAGE_SIZE = 40
/** Selected chips shown before collapsing behind a "+N more" toggle. */
const CHIP_LIMIT = 12

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

function highlight(text: string, tokens: string[]): React.ReactNode {
  if (tokens.length === 0) return text
  const pattern = new RegExp(`(${tokens.map(escapeRegExp).join('|')})`, 'gi')
  const matched = new Set(tokens)
  return text.split(pattern).map((part, index) =>
    matched.has(part.toLocaleLowerCase()) ? (
      <mark key={index} className="rounded-[2px] bg-primary/25 text-inherit">
        {part}
      </mark>
    ) : (
      <React.Fragment key={index}>{part}</React.Fragment>
    ),
  )
}

/**
 * Searchable multi-select for large entity sets (agents, skills). Selected
 * entries stay pinned as chips so they survive filtering, the option list is
 * height-bounded and rendered in pages, and the whole list is reachable from
 * the search field with the arrow keys.
 */
export function EntityMultiSelect({
  items,
  selectedIds,
  onChange,
  label,
  searchPlaceholder,
  emptyText,
  namePrefix = '',
  searchThreshold = 8,
  id,
  className,
}: EntityMultiSelectProps) {
  const { t } = useTranslation('common')
  const generatedId = React.useId()
  const rootId = id ?? generatedId
  const listId = `${rootId}-options`
  const listRef = React.useRef<HTMLDivElement>(null)
  const [query, setQuery] = React.useState('')
  const [selectedOnly, setSelectedOnly] = React.useState(false)
  const [activeIndex, setActiveIndex] = React.useState(-1)
  const [visibleCount, setVisibleCount] = React.useState(PAGE_SIZE)
  const [chipsExpanded, setChipsExpanded] = React.useState(false)

  const selectedSet = React.useMemo(() => new Set(selectedIds), [selectedIds])
  const showSearch = items.length > searchThreshold

  const sorted = React.useMemo(
    () => [...items].sort((a, b) => a.name.localeCompare(b.name)),
    [items],
  )

  const tokens = React.useMemo(
    () => query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean),
    [query],
  )

  const filtered = React.useMemo(
    () =>
      sorted.filter((item) => {
        if (selectedOnly && !selectedSet.has(item.id)) return false
        if (tokens.length === 0) return true
        const haystack = [item.name, item.description, ...(item.keywords ?? [])]
          .filter(Boolean)
          .join(' ')
          .toLocaleLowerCase()
        return tokens.every((token) => haystack.includes(token))
      }),
    [selectedOnly, selectedSet, sorted, tokens],
  )

  const selectedItems = React.useMemo(
    () => sorted.filter((item) => selectedSet.has(item.id)),
    [selectedSet, sorted],
  )

  const orphanIds = React.useMemo(() => {
    const known = new Set(items.map((item) => item.id))
    return selectedIds.filter((selectedId) => !known.has(selectedId))
  }, [items, selectedIds])

  React.useEffect(() => {
    setVisibleCount(PAGE_SIZE)
    setActiveIndex(-1)
    if (listRef.current) listRef.current.scrollTop = 0
  }, [query, selectedOnly])

  React.useEffect(() => {
    if (selectedOnly && selectedItems.length === 0) setSelectedOnly(false)
  }, [selectedItems.length, selectedOnly])

  const visible = filtered.slice(0, visibleCount)

  const toggle = React.useCallback(
    (item: EntityMultiSelectItem) => {
      if (item.disabled) return
      onChange(
        selectedSet.has(item.id)
          ? selectedIds.filter((selectedId) => selectedId !== item.id)
          : [...selectedIds, item.id],
      )
    },
    [onChange, selectedIds, selectedSet],
  )

  const moveActive = (delta: number) => {
    // ArrowUp before entering the list stays put: wrapping to the tail would
    // force the whole catalog to render just to scroll one option into view.
    if (filtered.length === 0 || (activeIndex < 0 && delta < 0)) return
    const next = Math.min(Math.max(activeIndex + delta, 0), filtered.length - 1)
    setActiveIndex(next)
    if (next >= visibleCount) setVisibleCount(Math.min(next + PAGE_SIZE, filtered.length))
  }

  React.useEffect(() => {
    if (activeIndex < 0) return
    listRef.current
      ?.querySelector(`[data-option-index="${activeIndex}"]`)
      ?.scrollIntoView({ block: 'nearest' })
  }, [activeIndex, visibleCount])

  const handleSearchKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      moveActive(1)
      return
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      moveActive(-1)
      return
    }
    if (event.key === 'Enter') {
      // Always swallow Enter: this field lives inside the agent form.
      event.preventDefault()
      const item = filtered[activeIndex]
      if (item) toggle(item)
      return
    }
    if (event.key === 'Escape' && query) {
      event.preventDefault()
      setQuery('')
    }
  }

  const handleScroll = (event: React.UIEvent<HTMLDivElement>) => {
    const target = event.currentTarget
    if (target.scrollHeight - target.scrollTop - target.clientHeight > 96) return
    setVisibleCount((count) => (count >= filtered.length ? count : Math.min(count + PAGE_SIZE, filtered.length)))
  }

  if (items.length === 0 && orphanIds.length === 0) {
    return <p className={cn('text-[11px] text-muted-foreground', className)}>{emptyText}</p>
  }

  const chips = chipsExpanded ? selectedItems : selectedItems.slice(0, CHIP_LIMIT)
  const hiddenChipCount = selectedItems.length - chips.length
  // A short catalog is already fully visible; only the option rows earn their space.
  const showChips =
    (showSearch || orphanIds.length > 0) && (selectedItems.length > 0 || orphanIds.length > 0)

  return (
    <div className={cn('space-y-2', className)}>
      {showSearch && (
        <div className="flex flex-wrap items-center gap-2">
          <div className="relative min-w-48 flex-1">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              id={rootId}
              role="combobox"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={handleSearchKeyDown}
              placeholder={searchPlaceholder}
              aria-label={searchPlaceholder}
              aria-controls={listId}
              aria-expanded
              aria-activedescendant={
                activeIndex >= 0 ? `${listId}-option-${activeIndex}` : undefined
              }
              autoComplete="off"
              className="h-8 pl-8 pr-8 text-xs"
            />
            {query && (
              <button
                type="button"
                aria-label={t('multiSelect.clearSearch')}
                onClick={() => setQuery('')}
                className="absolute right-1 top-1/2 grid h-6 w-6 -translate-y-1/2 place-items-center rounded-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
              >
                <X className="h-3.5 w-3.5" />
              </button>
            )}
          </div>
          <div className="flex shrink-0 items-center gap-1 rounded-md border border-border bg-background p-0.5">
            {([false, true] as const).map((mode) => (
              <button
                key={String(mode)}
                type="button"
                aria-pressed={selectedOnly === mode}
                disabled={mode && selectedItems.length === 0}
                onClick={() => setSelectedOnly(mode)}
                className={cn(
                  'rounded-[5px] px-2 py-1 text-[11px] font-medium transition-colors',
                  selectedOnly === mode
                    ? 'bg-primary/15 text-foreground'
                    : 'text-muted-foreground hover:text-foreground',
                  mode && selectedItems.length === 0 && 'cursor-not-allowed opacity-50 hover:text-muted-foreground',
                )}
              >
                {mode
                  ? t('multiSelect.filterSelected', { count: selectedItems.length + orphanIds.length })
                  : t('multiSelect.filterAll', { count: items.length })}
              </button>
            ))}
          </div>
        </div>
      )}

      {showChips && (
        <div className="flex flex-wrap items-center gap-1.5 rounded-md border border-border bg-muted/40 p-2">
          {chips.map((item) => (
            <span
              key={item.id}
              className="inline-flex max-w-full items-center gap-1 rounded-md border border-primary/40 bg-primary/10 py-0.5 pl-2 pr-1 text-[11px] font-medium"
            >
              <span className="truncate">{`${namePrefix}${item.name}`}</span>
              <button
                type="button"
                aria-label={t('multiSelect.remove', { name: item.name })}
                onClick={() => toggle(item)}
                className="grid h-4 w-4 shrink-0 place-items-center rounded-sm text-muted-foreground transition-colors hover:bg-primary/20 hover:text-foreground"
              >
                <X className="h-3 w-3" />
              </button>
            </span>
          ))}
          {orphanIds.map((orphanId) => (
            <span
              key={orphanId}
              title={t('multiSelect.missingHint')}
              className="inline-flex max-w-full items-center gap-1 rounded-md border border-warning-foreground/40 bg-warning/55 py-0.5 pl-2 pr-1 text-[11px] font-medium text-warning-foreground"
            >
              <span className="truncate">{t('multiSelect.missing', { id: orphanId.slice(0, 8) })}</span>
              <button
                type="button"
                aria-label={t('multiSelect.remove', { name: orphanId })}
                onClick={() => onChange(selectedIds.filter((selectedId) => selectedId !== orphanId))}
                className="grid h-4 w-4 shrink-0 place-items-center rounded-sm transition-colors hover:bg-warning-foreground/20"
              >
                <X className="h-3 w-3" />
              </button>
            </span>
          ))}
          {hiddenChipCount > 0 && (
            <button
              type="button"
              onClick={() => setChipsExpanded(true)}
              className="rounded-md px-1.5 py-0.5 text-[11px] font-medium text-muted-foreground underline-offset-2 transition-colors hover:text-foreground hover:underline"
            >
              {t('multiSelect.moreChips', { count: hiddenChipCount })}
            </button>
          )}
          <button
            type="button"
            onClick={() => {
              onChange([])
              setChipsExpanded(false)
            }}
            className="ml-auto rounded-md px-1.5 py-0.5 text-[11px] font-medium text-muted-foreground underline-offset-2 transition-colors hover:text-destructive hover:underline"
          >
            {t('multiSelect.clearAll')}
          </button>
        </div>
      )}

      <div
        ref={listRef}
        id={listId}
        role="listbox"
        aria-label={label}
        aria-multiselectable
        onScroll={handleScroll}
        className={cn(
          'overflow-y-auto rounded-md border border-border bg-background p-1',
          showSearch && 'max-h-64',
        )}
      >
        {filtered.length === 0 ? (
          <p className="px-2 py-3 text-[11px] text-muted-foreground">
            {selectedOnly ? t('multiSelect.noSelection') : t('state.noMatches')}
          </p>
        ) : (
          visible.map((item, index) => {
            const checked = selectedSet.has(item.id)
            return (
              <button
                key={item.id}
                id={`${listId}-option-${index}`}
                data-option-index={index}
                type="button"
                role="option"
                // With a search field the arrow keys drive the list, so the rows
                // stay out of the tab order; small lists have no search field.
                tabIndex={showSearch ? -1 : 0}
                aria-selected={checked}
                aria-disabled={item.disabled}
                title={item.disabled ? item.disabledReason ?? undefined : undefined}
                onClick={() => toggle(item)}
                onMouseEnter={() => setActiveIndex(index)}
                className={cn(
                  'flex w-full items-start gap-2 rounded-sm px-2 py-1.5 text-left outline-none transition-colors',
                  index === activeIndex ? 'bg-accent text-accent-foreground' : 'hover:bg-muted',
                  item.disabled && 'cursor-not-allowed opacity-50 hover:bg-transparent',
                )}
              >
                <span
                  aria-hidden
                  className={cn(
                    'mt-0.5 grid h-4 w-4 shrink-0 place-items-center rounded-[4px] border transition-colors',
                    checked ? 'border-primary bg-primary text-primary-foreground' : 'border-border',
                  )}
                >
                  {checked && <Check className="h-3 w-3" />}
                </span>
                <span className="min-w-0 flex-1">
                  <span className="flex items-center gap-1.5">
                    <span className="truncate text-xs font-medium">
                      {namePrefix}
                      {highlight(item.name, tokens)}
                    </span>
                    {item.badge && (
                      <span className="shrink-0 rounded-sm bg-muted px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide text-muted-foreground">
                        {item.badge}
                      </span>
                    )}
                  </span>
                  {item.description && (
                    <span className="block truncate text-[11px] text-muted-foreground">
                      {highlight(item.description, tokens)}
                    </span>
                  )}
                </span>
              </button>
            )
          })
        )}
      </div>

      {showSearch && filtered.length > 0 && (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 px-0.5 text-[11px] text-muted-foreground">
          <span role="status">
            {t('multiSelect.showing', { shown: visible.length, total: filtered.length })}
          </span>
          {visible.length < filtered.length && (
            <button
              type="button"
              onClick={() => setVisibleCount((count) => Math.min(count + PAGE_SIZE, filtered.length))}
              className="font-medium underline-offset-2 transition-colors hover:text-foreground hover:underline"
            >
              {t('multiSelect.showMore')}
            </button>
          )}
          {tokens.length > 0 && filtered.some((item) => !selectedSet.has(item.id) && !item.disabled) && (
            <button
              type="button"
              onClick={() =>
                onChange([
                  ...selectedIds,
                  ...filtered
                    .filter((item) => !selectedSet.has(item.id) && !item.disabled)
                    .map((item) => item.id),
                ])
              }
              className="ml-auto font-medium underline-offset-2 transition-colors hover:text-foreground hover:underline"
            >
              {t('multiSelect.selectMatches', {
                count: filtered.filter((item) => !selectedSet.has(item.id) && !item.disabled).length,
              })}
            </button>
          )}
        </div>
      )}
    </div>
  )
}
