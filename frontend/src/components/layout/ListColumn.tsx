import { useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react'
import { Link, NavLink, useLocation, useNavigate } from 'react-router-dom'
import { Copy, MoreHorizontal, Pencil, Plus, Search, Trash2, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { OverlayLink } from '@/components/layout/overlayRouting'
import { VerticalResizeHandle } from '@/components/layout/VerticalResizeHandle'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { PageState } from '@/components/ui/page-state'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { SkeletonList } from '@/components/ui/skeleton'
import { Input } from '@/components/ui/input'
import { usePersistentPaneWidth } from '@/hooks/usePersistentPaneWidth'
import { navItemClass } from '@/lib/navItemClass'
import { cn } from '@/lib/utils'

export interface ListColumnItem {
  id: string
  to: string
  name: string
  summary: string
  avatarClass: string
  avatarInitial: string
  deleteTitle?: string
  deleteDescription?: string
}

export interface ListColumnProps {
  title: string
  newTo: string
  newLabel: string
  searchPlaceholder: string
  isLoading: boolean
  loadError: boolean
  errorText: string
  emptyText: string
  items: ListColumnItem[]
  /** Optional per-item icon rendered in the row's avatar slot. */
  icon?: typeof Search
  className?: string
  onRename?: (item: ListColumnItem, name: string) => Promise<unknown>
  onDelete?: (item: ListColumnItem) => Promise<unknown>
}

/** One width for every area, remembered across areas and across sessions. */
const WIDTH_STORAGE_KEY = 'ag-swarmer:layout:library-list-width'
const DEFAULT_WIDTH = 260
const MIN_WIDTH = 220
const MAX_WIDTH = 360

/**
 * Generic entity list column (the left pane of every resource-library area):
 * sticky header with a primary "new" action, a local search filter with a
 * live match count, avatar/name/summary rows highlighted by route match, and
 * skeleton / error / empty states so the column never collapses into a blank
 * strip while its query is in flight.
 *
 * The header's create action is the area's only one while there is a list to
 * add to; once the list is empty the empty state's own call to action takes
 * over, because two identical buttons a few pixels apart is not a choice.
 */
export function ListColumn({
  title,
  newTo,
  newLabel,
  searchPlaceholder,
  isLoading,
  loadError,
  errorText,
  emptyText,
  items,
  icon: ItemIcon,
  className,
  onRename,
  onDelete,
}: ListColumnProps) {
  const { t } = useTranslation('common')
  const location = useLocation()
  const navigate = useNavigate()
  const [query, setQuery] = useState('')
  const [openMenuId, setOpenMenuId] = useState<string | null>(null)
  const [renameTarget, setRenameTarget] = useState<ListColumnItem | null>(null)
  const [renameValue, setRenameValue] = useState('')
  const [renameError, setRenameError] = useState<string | null>(null)
  const [renaming, setRenaming] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<ListColumnItem | null>(null)
  const [copiedId, setCopiedId] = useState<string | null>(null)
  const rootRef = useRef<HTMLDivElement>(null)
  const pane = usePersistentPaneWidth({
    storageKey: WIDTH_STORAGE_KEY,
    defaultWidth: DEFAULT_WIDTH,
    minWidth: MIN_WIDTH,
    maxWidth: MAX_WIDTH,
  })
  const q = query.trim().toLowerCase()
  const filtered = useMemo(
    () =>
      q
        ? items.filter(
            (item) =>
              item.name.toLowerCase().includes(q) ||
              item.summary.toLowerCase().includes(q),
          )
        : items,
    [items, q],
  )
  // Only genuinely empty, not "empty because the query is still in flight":
  // dropping the header action during loading would make it pop in afterwards.
  const isEmpty = !isLoading && !loadError && items.length === 0

  return (
    <div
      ref={rootRef}
      className={cn('flex h-full shrink-0 max-lg:!w-full', className)}
      style={{ width: pane.width }}
    >
      <div className="flex min-w-0 flex-1 flex-col bg-card/40">
        {/* Header: area title left, primary create action right. */}
        <div className="flex h-14 shrink-0 items-center justify-between gap-2 border-b border-border px-3">
          <h2 className="truncate text-sm font-semibold tracking-tight">{title}</h2>
          {isEmpty ? null : (
            <Button
              size="sm"
              variant="default"
              className="h-7 gap-1 px-2.5 text-xs shadow-xs"
              asChild
            >
              <Link to={newTo} aria-label={newLabel}>
                <Plus className="h-3.5 w-3.5" />
                <span className="hidden lg:inline">{newLabel}</span>
              </Link>
            </Button>
          )}
        </div>

        {/* Search filter with a clear button and a reserved-height match count so
            results appearing never shift the list downward. */}
        <div className="shrink-0 px-3 pb-2 pt-2.5">
          <div className="relative">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              // Down from the box moves into the results, so filtering and
              // picking is one gesture without reaching for the mouse.
              onKeyDown={(event) => {
                if (event.key !== 'ArrowDown') return
                event.preventDefault()
                rootRef.current
                  ?.querySelectorAll<HTMLElement>('[data-list-row]')[0]
                  ?.focus()
              }}
              placeholder={searchPlaceholder}
              aria-label={searchPlaceholder}
              className="h-8 rounded-lg bg-background pl-8 pr-8 text-xs"
            />
            {query ? (
              <button
                type="button"
                aria-label={t('state.clearSearch')}
                onClick={() => setQuery('')}
                className="absolute right-1.5 top-1/2 flex h-5 w-5 -translate-y-1/2 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
              >
                <X className="h-3 w-3" />
              </button>
            ) : null}
          </div>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-2 pb-2">
          {isLoading ? (
            <SkeletonList rows={7} />
          ) : loadError ? (
            <PageState inset variant="error" icon={null} title={errorText} />
          ) : items.length === 0 ? (
            <PageState
              inset
              icon={ItemIcon}
              title={emptyText}
              description={t('state.emptyHint')}
              action={
                <Button size="sm" variant="default" asChild>
                  <OverlayLink to={newTo}>
                    <Plus className="h-3.5 w-3.5" />
                    {newLabel}
                  </OverlayLink>
                </Button>
              }
            />
          ) : filtered.length === 0 ? (
            <PageState inset icon={Search} title={t('state.noMatches')} />
          ) : (
            <ul className="space-y-0.5" aria-label={title} onKeyDown={onRowKeys}>
              {filtered.map((item) => (
                <li key={item.id} className="group/row relative">
                  <NavLink
                    to={item.to}
                    title={`${item.name} — ${item.summary}`}
                    data-list-row=""
                    className={({ isActive }) =>
                      navItemClass(isActive, 'items-start gap-2.5 py-1.5 pl-2.5 pr-9')
                    }
                  >
                    {({ isActive }) => (
                      <>
                        <span
                          aria-hidden
                          className={cn(
                            'mt-0.5 flex h-8 w-8 shrink-0 select-none items-center justify-center rounded-full text-xs font-semibold',
                            item.avatarClass,
                          )}
                        >
                          {ItemIcon ? (
                            <ItemIcon className="h-4 w-4" />
                          ) : (
                            item.avatarInitial
                          )}
                        </span>
                        <div className="flex min-w-0 flex-1 flex-col gap-0.5 pt-0.5">
                          <span
                            className={cn(
                              'truncate text-sm leading-tight',
                              isActive ? 'font-semibold' : 'font-medium',
                            )}
                          >
                            {item.name}
                          </span>
                          <p className="truncate text-xs font-normal leading-tight text-muted-foreground group-hover:text-foreground/70">
                            {item.summary}
                          </p>
                        </div>
                      </>
                    )}
                  </NavLink>
                  <Popover
                    open={openMenuId === item.id}
                    onOpenChange={(open) => setOpenMenuId(open ? item.id : null)}
                  >
                    <PopoverTrigger asChild>
                      <button
                        type="button"
                        aria-label={t('entityMenu.actionsLabel', { name: item.name })}
                        className="absolute right-1.5 top-1/2 flex h-7 w-7 -translate-y-1/2 items-center justify-center rounded-md text-muted-foreground opacity-60 outline-none transition hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring sm:opacity-0 sm:group-hover/row:opacity-100 sm:group-focus-within/row:opacity-100"
                      >
                        <MoreHorizontal className="h-4 w-4" />
                      </button>
                    </PopoverTrigger>
                    <PopoverContent
                      role="menu"
                      aria-label={t('entityMenu.actionsLabel', { name: item.name })}
                      side="right"
                      align="start"
                      onKeyDown={onMenuKeys}
                    >
                      {onRename ? (
                        <button
                          type="button"
                          role="menuitem"
                          className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm outline-none hover:bg-accent focus-visible:bg-accent"
                          onClick={() => {
                            setOpenMenuId(null)
                            setRenameTarget(item)
                            setRenameValue(item.name)
                            setRenameError(null)
                          }}
                        >
                          <Pencil className="h-3.5 w-3.5" />
                          {t('entityMenu.rename')}
                        </button>
                      ) : null}
                      <button
                        type="button"
                        role="menuitem"
                        className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm outline-none hover:bg-accent focus-visible:bg-accent"
                        onClick={() => {
                          setOpenMenuId(null)
                          if (!navigator.clipboard) return
                          void navigator.clipboard
                            .writeText(item.id)
                            .then(() => setCopiedId(item.id))
                            .catch(() => undefined)
                        }}
                      >
                        <Copy className="h-3.5 w-3.5" />
                        {t('entityMenu.copyId')}
                      </button>
                      {onDelete ? (
                        <>
                          <div className="my-1 border-t border-border" role="separator" />
                          <button
                            type="button"
                            role="menuitem"
                            className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm text-destructive outline-none hover:bg-accent focus-visible:bg-accent"
                            onClick={() => {
                              setOpenMenuId(null)
                              setDeleteTarget(item)
                            }}
                          >
                            <Trash2 className="h-3.5 w-3.5" />
                            {t('actions.delete')}
                          </button>
                        </>
                      ) : null}
                    </PopoverContent>
                  </Popover>
                </li>
              ))}
            </ul>
          )}
        </div>

        {/* Sticky footer count — a quiet "how many do I have" without counting
            rows by eye. Doubles as the live region for filter results. */}
        {!isLoading && !loadError && items.length > 0 ? (
          <div
            aria-live="polite"
            className="shrink-0 border-t border-border px-3 py-1.5 text-2xs text-muted-foreground tabular-nums"
          >
            {q
              ? filtered.length > 0
                ? t('state.matchCount', { count: filtered.length })
                : t('state.noMatches')
              : t('state.totalCount', { count: items.length })}
          </div>
        ) : null}
      </div>
      {/* Doubles as the column's right border: the handle already paints a
          hairline down its centre. */}
      <VerticalResizeHandle
        label={t('state.resizeList')}
        value={pane.width}
        min={pane.minWidth}
        max={pane.maxWidth}
        onResizeStart={pane.startResize}
        onStep={pane.resizeBy}
        className="max-lg:hidden"
      />
      <span className="sr-only" role="status">
        {copiedId ? t('entityMenu.copiedId') : ''}
      </span>
      <Dialog
        open={renameTarget !== null}
        onOpenChange={(open) => {
          if (!open && !renaming) setRenameTarget(null)
        }}
      >
        <DialogContent closeLabel={t('actions.close')} className="sm:max-w-sm">
          <form
            className="space-y-4"
            onSubmit={async (event) => {
              event.preventDefault()
              const name = renameValue.trim()
              if (!renameTarget || !name) return
              setRenaming(true)
              setRenameError(null)
              try {
                await onRename?.(renameTarget, name)
                setRenameTarget(null)
              } catch (error) {
                setRenameError(error instanceof Error ? error.message : String(error))
              } finally {
                setRenaming(false)
              }
            }}
          >
            <DialogHeader>
              <DialogTitle>
                {t('entityMenu.renameTitle', { name: renameTarget?.name ?? '' })}
              </DialogTitle>
              <DialogDescription>{t('entityMenu.renameDescription')}</DialogDescription>
            </DialogHeader>
            <Input
              autoFocus
              maxLength={100}
              value={renameValue}
              onChange={(event) => setRenameValue(event.target.value)}
              aria-label={t('entityMenu.name')}
              aria-invalid={renameError !== null}
            />
            {renameError ? (
              <p className="text-xs text-destructive" role="alert">{renameError}</p>
            ) : null}
            <DialogFooter>
              <Button
                type="button"
                variant="outline"
                disabled={renaming}
                onClick={() => setRenameTarget(null)}
              >
                {t('actions.cancel')}
              </Button>
              <Button type="submit" disabled={renaming || !renameValue.trim()}>
                {renaming ? t('actions.saving') : t('actions.save')}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
      <ConfirmDialog
        open={deleteTarget !== null}
        onOpenChange={(open) => {
          if (!open) setDeleteTarget(null)
        }}
        title={deleteTarget?.deleteTitle ?? t('entityMenu.deleteTitle', { name: deleteTarget?.name ?? '' })}
        description={deleteTarget?.deleteDescription ?? t('entityMenu.deleteDescription')}
        confirmLabel={t('actions.delete')}
        destructive
        onConfirm={async () => {
          const item = deleteTarget
          if (!item) return
          await onDelete?.(item)
          if (location.pathname === item.to) {
            void navigate(newTo.replace(/\/new$/, ''), { replace: true })
          }
          setDeleteTarget(null)
        }}
      />
    </div>
  )
}

/**
 * Arrow-key roving through the rows. Enter is the link's own job, so this only
 * has to move the focus that Enter then acts on.
 */
function onRowKeys(event: ReactKeyboardEvent<HTMLUListElement>) {
  if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return
  event.preventDefault()
  const rows = Array.from(event.currentTarget.querySelectorAll<HTMLElement>('[data-list-row]'))
  if (rows.length === 0) return
  const current = rows.indexOf(document.activeElement as HTMLElement)
  const next =
    event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? rows.length - 1
        : event.key === 'ArrowDown'
          ? (current + 1) % rows.length
          : (current - 1 + rows.length) % rows.length
  rows[next]?.focus()
}

function onMenuKeys(event: ReactKeyboardEvent<HTMLDivElement>) {
  if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return
  event.preventDefault()
  const items = Array.from(
    event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
  )
  const current = items.indexOf(document.activeElement as HTMLButtonElement)
  const next = event.key === 'Home'
    ? 0
    : event.key === 'End'
      ? items.length - 1
      : event.key === 'ArrowDown'
        ? (current + 1) % items.length
        : (current - 1 + items.length) % items.length
  items[next]?.focus()
}
