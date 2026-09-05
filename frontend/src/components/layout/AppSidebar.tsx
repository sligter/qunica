import { lazy, Suspense, useEffect, useRef, useState, type ReactNode } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { NavLink, useLocation, useNavigate } from 'react-router-dom'
import {
  ChevronDown,
  Command,
  Library,
  LogOut,
  MessageSquarePlus,
  PanelLeft,
  PanelLeftClose,
  Pencil,
  Search,
  Settings,
  Trash2,
  X,
} from 'lucide-react'

import { formatRelativeTime } from '@/lib/format'
import { setConversationIdDrag } from '@/lib/conversationDrag'
import { cn } from '@/lib/utils'
import { normalizeLanguage } from '@/i18n'
import { useGroups } from '@/hooks/useGroups'
import { useConversationPrefetch } from '@/hooks/useGroupMessages'
import {
  useDeleteDirectChat,
  useDirectChats,
  useRenameDirectChat,
} from '@/hooks/useDirectChats'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { BrandMark } from '@/components/brand/BrandMark'
import { ConversationStatusIndicator } from '@/components/chat/ConversationStatusDot'
import { AgentAvatar } from '@/components/chat/AgentAvatar'
import { OverlayNavLink, useOverlayLinkState } from '@/components/layout/overlayRouting'
import {
  commandShortcutHint,
  useOpenCommandPalette,
} from '@/components/layout/CommandPalette'
import { useAuthStore } from '@/stores/authStore'
import { isDesktopRuntime } from '@/lib/runtime'
import { openLibraryWindow, openSettingsWindow } from '@/lib/desktop'
import { DirectChatPickerDialog } from '@/components/direct-chats/DirectChatPickerDialog'
import { GroupAvatar } from '@/components/groups/GroupAvatar'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { logTerminalCleanupError } from '@/terminal/logTerminalCleanupError'
import { useTerminalRuntime } from '@/terminal/TerminalRuntimeProvider'

const COLLAPSED_KEY = 'qunica:layout:sidebar-collapsed'
const LIST_BATCH_SIZE = 20

function useLazyList(total: number, active: boolean, resetKey: string) {
  const [limit, setLimit] = useState(LIST_BATCH_SIZE)
  const sentinelRef = useRef<HTMLLIElement>(null)

  useEffect(() => setLimit(LIST_BATCH_SIZE), [resetKey])
  useEffect(() => {
    const sentinel = sentinelRef.current
    if (!active || limit >= total || !sentinel || typeof IntersectionObserver === 'undefined') {
      return
    }
    const observer = new IntersectionObserver(([entry]) => {
      if (entry?.isIntersecting) {
        setLimit((current) => Math.min(total, current + LIST_BATCH_SIZE))
      }
    }, { rootMargin: '160px' })
    observer.observe(sentinel)
    return () => observer.disconnect()
  }, [active, limit, total])

  return {
    hasMore: limit < total,
    limit: Math.min(limit, total),
    loadMore: () => setLimit((current) => Math.min(total, current + LIST_BATCH_SIZE)),
    sentinelRef,
  }
}

function SidebarSectionHeader({
  action,
  controls,
  expanded,
  label,
  onToggle,
}: {
  action?: ReactNode
  controls: string
  expanded: boolean
  label: ReactNode
  onToggle: () => void
}) {
  return (
    <div className="flex h-7 items-center justify-between px-1">
      <button
        type="button"
        className="flex min-w-0 flex-1 items-center gap-1 rounded-sm text-xs font-medium uppercase tracking-wider text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        aria-controls={controls}
        aria-expanded={expanded}
        onClick={onToggle}
      >
        <ChevronDown
          className={cn('h-3.5 w-3.5 shrink-0 transition-transform', !expanded && '-rotate-90')}
        />
        <span className="truncate">{label}</span>
      </button>
      {action}
    </div>
  )
}

// The create-group form drags in react-hook-form + zod; it downloads the first
// time the user asks for it rather than on every app boot.
const GroupFormDialog = lazy(() =>
  import('@/components/groups/GroupFormDialog').then((m) => ({ default: m.GroupFormDialog })),
)

/** Route prefixes that count as "inside the library", for the entry's state. */
const LIBRARY_PREFIXES = [
  '/usage',
  '/agents',
  '/providers',
  '/mcp-servers',
  '/skills',
  '/workspaces',
]

interface DirectChatMenuState {
  id: string
  title: string
  x: number
  y: number
  trigger: HTMLElement
}

function readCollapsed(): boolean {
  try {
    return localStorage.getItem(COLLAPSED_KEY) === 'true'
  } catch {
    return false
  }
}

function storeCollapsed(value: boolean): void {
  try {
    localStorage.setItem(COLLAPSED_KEY, String(value))
  } catch {
    // Layout preference persistence should not block the UI.
  }
}

/**
 * Unified app sidebar: New group, searchable group list, Library links into the
 * settings surface, and a bottom Settings row plus user menu. Collapses to a
 * narrow icon strip; the collapsed state persists in localStorage.
 */
export function AppSidebar({ mobile = false }: { mobile?: boolean } = {}) {
  const { t, i18n } = useTranslation(['navigation', 'groups', 'common'])
  const [desktopCollapsed, setCollapsed] = useState(readCollapsed)
  const collapsed = !mobile && desktopCollapsed
  const [dialogOpen, setDialogOpen] = useState(false)
  const [directDialogOpen, setDirectDialogOpen] = useState(false)
  const [searchOpen, setSearchOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [directChatsExpanded, setDirectChatsExpanded] = useState(true)
  const [groupsExpanded, setGroupsExpanded] = useState(true)
  const [chatMenu, setChatMenu] = useState<DirectChatMenuState | null>(null)
  const [pendingRenameChat, setPendingRenameChat] = useState<{ id: string; title: string } | null>(null)
  const [renameValue, setRenameValue] = useState('')
  const [renameError, setRenameError] = useState<string | null>(null)
  const [pendingDeleteChat, setPendingDeleteChat] = useState<{ id: string; title: string } | null>(null)
  const chatMenuRef = useRef<HTMLDivElement>(null)
  const chatMenuFirstItemRef = useRef<HTMLButtonElement>(null)
  const navigate = useNavigate()
  const prefetchConversation = useConversationPrefetch()
  // Carried on the library jump so the conversation underneath stays mounted,
  // exactly as the sidebar's OverlayLinks do.
  const overlayState = useOverlayLinkState()
  const location = useLocation()
  const groups = useGroups()
  const directChats = useDirectChats()
  const renameDirectChat = useRenameDirectChat(pendingRenameChat?.id ?? '')
  const deleteDirectChat = useDeleteDirectChat(pendingDeleteChat?.id ?? '')
  const { closeConversation } = useTerminalRuntime()

  const toggleCollapsed = () => {
    setCollapsed((current) => {
      const next = !current
      storeCollapsed(next)
      return next
    })
  }

  const q = query.trim().toLowerCase()
  const filteredGroups = (groups.data ?? []).filter(
    (g) =>
      !q ||
      g.name.toLowerCase().includes(q) ||
      (g.description ?? '').toLowerCase().includes(q),
  )
  const filteredDirectChats = (directChats.data ?? []).filter(
    (chat) =>
      !q ||
      chat.title.toLowerCase().includes(q) ||
      (chat.agent_name ?? '').toLowerCase().includes(q),
  )
  const directLazy = useLazyList(filteredDirectChats.length, directChatsExpanded, q)
  const groupLazy = useLazyList(filteredGroups.length, groupsExpanded, q)
  const visibleDirectChats = filteredDirectChats.slice(0, directLazy.limit)
  const visibleGroups = filteredGroups.slice(0, groupLazy.limit)
  const closeSearch = () => {
    setQuery('')
    setSearchOpen(false)
  }
  const openChatMenu = (
    x: number,
    y: number,
    chat: { id: string; title: string },
    trigger: HTMLElement,
  ) => {
    setChatMenu({
      ...chat,
      x: Math.max(8, Math.min(x, window.innerWidth - 184)),
      y: Math.max(8, Math.min(y, window.innerHeight - 88)),
      trigger,
    })
  }

  useEffect(() => {
    if (!chatMenu) return
    chatMenuFirstItemRef.current?.focus()
    const dismiss = (event: PointerEvent) => {
      if (event.target instanceof Node && chatMenuRef.current?.contains(event.target)) return
      setChatMenu(null)
    }
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      setChatMenu(null)
      chatMenu.trigger.focus()
    }
    window.addEventListener('pointerdown', dismiss)
    window.addEventListener('keydown', escape)
    return () => {
      window.removeEventListener('pointerdown', dismiss)
      window.removeEventListener('keydown', escape)
    }
  }, [chatMenu])

  // Whether the current route lives inside the library. Drives the entry
  // row's active state; the area itself is named only by the rail — the
  // entry always says "Library" and always lands on /agents.
  const libraryActive = LIBRARY_PREFIXES.some(
    (prefix) =>
      location.pathname === prefix || location.pathname.startsWith(`${prefix}/`),
  )
  // On the desktop shell the library and settings open as independent
  // top-level windows, so they can sit beside the conversation and be used at
  // the same time; the browser build keeps the in-app overlay.
  const desktop = isDesktopRuntime()

  return (
    <aside
      className={cn(
        'flex h-full shrink-0 flex-col border-r border-border bg-card',
        mobile ? 'w-full border-r-0' : collapsed ? 'w-14' : 'w-[248px]',
      )}
    >
      {dialogOpen ? (
        <Suspense fallback={null}>
          <GroupFormDialog open onOpenChange={setDialogOpen} />
        </Suspense>
      ) : null}
      <DirectChatPickerDialog open={directDialogOpen} onOpenChange={setDirectDialogOpen} />
      <Dialog
        open={pendingRenameChat !== null}
        onOpenChange={(open) => {
          if (!open && !renameDirectChat.isPending) {
            setPendingRenameChat(null)
            setRenameError(null)
          }
        }}
      >
        <DialogContent closeLabel={t('common:actions.close')} className="sm:max-w-sm">
          <form
            className="space-y-4"
            onSubmit={async (event) => {
              event.preventDefault()
              const title = renameValue.trim()
              if (!title || Array.from(title).length > 120) {
                setRenameError(t('chat:direct.titleInvalid'))
                return
              }
              try {
                setRenameError(null)
                await renameDirectChat.mutateAsync({ title })
                setPendingRenameChat(null)
              } catch (error) {
                setRenameError(error instanceof Error ? error.message : String(error))
              }
            }}
          >
            <DialogHeader>
              <DialogTitle>{t('chat:direct.rename')}</DialogTitle>
              <DialogDescription>{t('chat:direct.renameDescription')}</DialogDescription>
            </DialogHeader>
            <Input
              autoFocus
              maxLength={120}
              value={renameValue}
              onChange={(event) => setRenameValue(event.target.value)}
              aria-label={t('chat:direct.rename')}
              aria-invalid={renameError !== null}
            />
            {renameError ? <p role="alert" className="text-xs text-destructive">{renameError}</p> : null}
            <DialogFooter>
              <Button
                type="button"
                variant="outline"
                disabled={renameDirectChat.isPending}
                onClick={() => setPendingRenameChat(null)}
              >
                {t('common:actions.cancel')}
              </Button>
              <Button type="submit" disabled={renameDirectChat.isPending}>
                {t('common:actions.save')}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
      <ConfirmDialog
        open={pendingDeleteChat !== null}
        onOpenChange={(open) => { if (!open) setPendingDeleteChat(null) }}
        title={t('chat:direct.deleteTitle')}
        description={pendingDeleteChat ? t('chat:direct.deleteDescription', { title: pendingDeleteChat.title }) : undefined}
        confirmLabel={t('common:actions.delete')}
        destructive
        onConfirm={async () => {
          const chat = pendingDeleteChat
          if (!chat) return
          await deleteDirectChat.mutateAsync()
          await closeConversation(chat.id, true).catch(logTerminalCleanupError)
          if (location.pathname === `/chats/${chat.id}`) navigate('/', { replace: true })
          setPendingDeleteChat(null)
        }}
      />

      {/* Header: product name + collapse toggle */}
      <div
        className={cn(
          'flex h-14 shrink-0 items-center gap-2',
          collapsed ? 'justify-center' : 'px-3',
        )}
      >
        {!collapsed && (
          <>
            <BrandMark className="h-8 w-8" />
            <span className="min-w-0 flex-1 truncate font-serif text-sm font-semibold tracking-tight">
              Qunica
            </span>
          </>
        )}
        <Button
          variant="ghost"
          size="icon"
          onClick={toggleCollapsed}
          className={mobile ? 'hidden' : undefined}
          aria-label={t(collapsed ? 'navigation:expandSidebar' : 'navigation:collapseSidebar')}
        >
          {collapsed ? (
            <PanelLeft className="h-4 w-4" />
          ) : (
            <PanelLeftClose className="h-4 w-4" />
          )}
        </Button>
      </div>

      {/* New conversations */}
      <div className={cn('shrink-0', collapsed ? 'flex justify-center pb-2' : 'px-3 pb-2')}>
        {collapsed ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                size="icon"
                onClick={() => setDirectDialogOpen(true)}
                aria-label={t('navigation:newDirectChat')}
              >
                <MessageSquarePlus className="h-4 w-4" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="right">{t('navigation:newDirectChat')}</TooltipContent>
          </Tooltip>
        ) : (
          <div className="grid grid-cols-2 gap-2">
            <Button
              className="min-w-0 justify-center gap-1.5 rounded-lg px-2 text-xs"
              onClick={() => setDirectDialogOpen(true)}
            >
              <MessageSquarePlus className="h-4 w-4" />
              {t('navigation:newDirectChat')}
            </Button>
            <Button
              variant="outline"
              className="min-w-0 justify-center gap-1.5 rounded-lg px-2 text-xs"
              onClick={() => setDialogOpen(true)}
            >
              <MessageSquarePlus className="h-4 w-4" />
              {t('navigation:newGroup')}
            </Button>
          </div>
        )}
      </div>

      {/* Conversations */}
      {collapsed ? (
        <div className="min-h-0 flex-1" />
      ) : (
        <div className="flex min-h-0 flex-1 flex-col">
          {searchOpen ? (
            <div className="shrink-0 px-3 pt-2">
              <div className="relative">
                <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
                <Input
                  autoFocus
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Escape') closeSearch()
                  }}
                  placeholder={t('navigation:searchConversations')}
                  aria-label={t('navigation:searchConversations')}
                  className="h-8 px-8 text-xs"
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="absolute right-0.5 top-1/2 h-7 w-7 -translate-y-1/2"
                  aria-label={t('common:actions.close')}
                  onClick={closeSearch}
                >
                  <X className="h-3.5 w-3.5" />
                </Button>
              </div>
            </div>
          ) : null}
          <div className="chat-message-scrollbar min-h-0 flex-1 overflow-y-auto overscroll-contain px-2 py-2">
            <SidebarSectionHeader
              controls="sidebar-direct-chats"
              expanded={directChatsExpanded}
              label={t('navigation:directChats')}
              onToggle={() => setDirectChatsExpanded((current) => !current)}
              action={!searchOpen ? (
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="h-6 w-6"
                      aria-label={t('navigation:searchConversations')}
                      aria-expanded={searchOpen}
                      onClick={() => {
                        setDirectChatsExpanded(true)
                        setGroupsExpanded(true)
                        setSearchOpen(true)
                      }}
                    >
                      <Search className="h-3.5 w-3.5" />
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>{t('navigation:searchConversations')}</TooltipContent>
                </Tooltip>
              ) : undefined}
            />
            <div id="sidebar-direct-chats">
              {directChatsExpanded ? (
                <>
                  {directChats.isLoading ? (
                    <p className="px-2 pb-2 text-xs text-muted-foreground">{t('common:state.loading')}</p>
                  ) : null}
                  {directChats.error ? (
                    <p className="px-2 pb-2 text-xs text-destructive">{String(directChats.error)}</p>
                  ) : null}
                  <ul className="mb-3 space-y-0.5">
                    {visibleDirectChats.map((chat) => (
                      <li key={chat.id}>
                        <NavLink
                          to={`/chats/${chat.id}`}
                          draggable
                          onPointerEnter={() => prefetchConversation('direct-chats', chat.id)}
                          onFocus={() => prefetchConversation('direct-chats', chat.id)}
                          onDragStart={(event) => setConversationIdDrag(event.dataTransfer, chat.id)}
                          aria-haspopup="menu"
                          aria-controls={chatMenu?.id === chat.id ? 'direct-chat-context-menu' : undefined}
                          title={t('chat:direct.contextHint')}
                          onContextMenu={(event) => {
                            event.preventDefault()
                            openChatMenu(event.clientX, event.clientY, chat, event.currentTarget)
                          }}
                          onKeyDown={(event) => {
                            if (event.key !== 'ContextMenu' && !(event.shiftKey && event.key === 'F10')) return
                            event.preventDefault()
                            const rect = event.currentTarget.getBoundingClientRect()
                            openChatMenu(rect.left + 12, rect.top + 12, chat, event.currentTarget)
                          }}
                          className={({ isActive }) =>
                            cn(
                              'flex min-w-0 items-center gap-2.5 rounded-md px-2.5 py-2 text-left transition-colors',
                              isActive ? 'bg-primary/10' : 'hover:bg-card-hover',
                            )
                          }
                        >
                          {({ isActive }) => (
                            <>
                              <AgentAvatar
                                name={chat.agent_name ?? chat.title}
                                avatarUrl={chat.agent_avatar_url}
                                size="sm"
                              />
                              <span
                                className={cn(
                                  'min-w-0 flex-1 truncate text-sm',
                                  isActive ? 'font-semibold' : 'font-medium',
                                )}
                              >
                                {chat.title}
                              </span>
                              <ConversationStatusIndicator conversationId={chat.id} />
                              <span className="shrink-0 text-[10px] text-muted-foreground">
                                {formatRelativeTime(
                                  chat.updated_at,
                                  normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US',
                                )}
                              </span>
                            </>
                          )}
                        </NavLink>
                      </li>
                    ))}
                    {directLazy.hasMore ? (
                      <li ref={directLazy.sentinelRef}>
                        <button
                          type="button"
                          className="w-full rounded-md py-1.5 text-[11px] text-muted-foreground hover:bg-card-hover hover:text-foreground"
                          onClick={directLazy.loadMore}
                        >
                          {t('navigation:loadMore')}
                        </button>
                      </li>
                    ) : null}
                  </ul>
                </>
              ) : null}
            </div>

            <SidebarSectionHeader
              controls="sidebar-groups"
              expanded={groupsExpanded}
              label={t('navigation:groups')}
              onToggle={() => setGroupsExpanded((current) => !current)}
            />
            <div id="sidebar-groups">
              {groupsExpanded ? (
                <>
                  {groups.isLoading ? (
                    <p className="px-2 text-xs text-muted-foreground">{t('common:state.loading')}</p>
                  ) : null}
                  {groups.error ? (
                    <p className="px-2 text-xs text-destructive">{t('groups:loadError')}</p>
                  ) : null}
                  {groups.data && groups.data.length === 0 ? (
                    <p className="px-2 text-xs text-muted-foreground">{t('groups:empty')}</p>
                  ) : null}
                  {groups.data && groups.data.length > 0 && filteredGroups.length === 0 ? (
                    <p className="px-2 text-xs text-muted-foreground">{t('common:state.noMatches')}</p>
                  ) : null}
                  <ul className="space-y-0.5">
                    {visibleGroups.map((g) => (
                      <li key={g.id}>
                        <NavLink
                          to={`/groups/${g.id}`}
                          draggable
                          onPointerEnter={() => prefetchConversation('groups', g.id)}
                          onFocus={() => prefetchConversation('groups', g.id)}
                          onDragStart={(event) => setConversationIdDrag(event.dataTransfer, g.id)}
                          className={({ isActive }) =>
                            cn(
                              'flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left transition-colors',
                              isActive ? 'bg-primary/10' : 'hover:bg-card-hover',
                            )
                          }
                        >
                          {({ isActive }) => (
                            <>
                              <GroupAvatar
                                name={g.name}
                                avatarUrl={g.avatar_url}
                                members={g.avatar_members ?? []}
                                size="sm"
                              />
                              <span
                                className={cn(
                                  'min-w-0 flex-1 truncate text-sm',
                                  isActive ? 'font-semibold' : 'font-medium',
                                )}
                              >
                                {g.name}
                              </span>
                              <ConversationStatusIndicator conversationId={g.id} />
                              <span className="shrink-0 text-[10px] text-muted-foreground">
                                {formatRelativeTime(
                                  g.created_at,
                                  normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US',
                                )}
                              </span>
                            </>
                          )}
                        </NavLink>
                      </li>
                    ))}
                    {groupLazy.hasMore ? (
                      <li ref={groupLazy.sentinelRef}>
                        <button
                          type="button"
                          className="w-full rounded-md py-1.5 text-[11px] text-muted-foreground hover:bg-card-hover hover:text-foreground"
                          onClick={groupLazy.loadMore}
                        >
                          {t('navigation:loadMore')}
                        </button>
                      </li>
                    ) : null}
                  </ul>
                </>
              ) : null}
            </div>
          </div>
        </div>
      )}

      {chatMenu ? (
        <div
          ref={chatMenuRef}
          id="direct-chat-context-menu"
          role="menu"
          aria-label={t('chat:direct.contextMenu')}
          className="fixed z-50 min-w-44 overflow-hidden rounded-md border border-border bg-background py-1 text-sm text-foreground shadow-md"
          style={{ left: chatMenu.x, top: chatMenu.y }}
          onKeyDown={(event) => {
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
          }}
        >
          <button
            ref={chatMenuFirstItemRef}
            type="button"
            role="menuitem"
            className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted"
            onClick={() => {
              setPendingRenameChat({ id: chatMenu.id, title: chatMenu.title })
              setRenameValue(chatMenu.title)
              setRenameError(null)
              setChatMenu(null)
            }}
          >
            <Pencil className="h-3.5 w-3.5" />
            {t('chat:direct.rename')}
          </button>
          <div className="my-1 border-t border-border" role="separator" />
          <button
            type="button"
            role="menuitem"
            className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-destructive hover:bg-muted"
            onClick={() => {
              setPendingDeleteChat({ id: chatMenu.id, title: chatMenu.title })
              setChatMenu(null)
            }}
          >
            <Trash2 className="h-3.5 w-3.5" />
            {t('common:actions.delete')}
          </button>
        </div>
      ) : null}

      {/* Library — one entry, straight into the panel. The flyout menu that
          used to live here duplicated the ResourceRail 1:1 (same targets, same
          arrow keys); the rail is now the only place that names the areas.
          Landing on /agents gives the click a predictable target — the rail's
          first item — and switching areas from there is a single click. */}
      <nav
        className={cn(
          'shrink-0 border-t border-border py-2',
          collapsed ? 'flex flex-col items-center' : 'px-2',
        )}
      >
        {collapsed ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                onClick={() =>
                  desktop
                    ? void openLibraryWindow().catch(() => undefined)
                    : navigate('/agents', { state: overlayState })
                }
                aria-label={t('navigation:library')}
                aria-current={libraryActive ? 'page' : undefined}
                className={cn(
                  'flex h-8 w-8 items-center justify-center rounded-md transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
                  libraryActive
                    ? 'bg-primary/10 text-primary'
                    : 'text-muted-foreground hover:bg-card-hover hover:text-foreground',
                )}
              >
                <Library className="h-4 w-4" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">{t('navigation:library')}</TooltipContent>
          </Tooltip>
        ) : (
          <button
            type="button"
            onClick={() =>
              desktop
                ? void openLibraryWindow().catch(() => undefined)
                : navigate('/agents', { state: overlayState })
            }
            aria-current={libraryActive ? 'page' : undefined}
            className={cn(
              'flex w-full items-center gap-2.5 rounded-md px-3 py-1.5 text-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
              libraryActive
                ? 'bg-primary/10 font-medium text-primary'
                : 'text-muted-foreground hover:bg-card-hover hover:text-foreground',
            )}
          >
            <Library className="h-4 w-4 shrink-0" />
            <span className="min-w-0 flex-1 truncate text-left">
              {t('navigation:library')}
            </span>
          </button>
        )}
      </nav>

      {/* Bottom: ⌘K launcher above Settings + user menu */}
      <div
        className={cn(
          'shrink-0 border-t border-border py-2',
          collapsed ? 'flex flex-col items-center gap-1' : 'px-2',
        )}
      >
        <CommandTrigger collapsed={collapsed} />
        {collapsed ? (
          <Tooltip>
            <TooltipTrigger asChild>
              {desktop ? (
                <button
                  type="button"
                  onClick={() => void openSettingsWindow().catch(() => undefined)}
                  aria-label={t('navigation:settings')}
                  className="flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-card-hover hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                >
                  <Settings className="h-4 w-4" />
                </button>
              ) : (
                <OverlayNavLink
                  to="/settings"
                  aria-label={t('navigation:settings')}
                  className={({ isActive }) =>
                    cn(
                      'flex h-8 w-8 items-center justify-center rounded-md transition-colors',
                      isActive
                        ? 'bg-primary/10 text-primary'
                        : 'text-muted-foreground hover:bg-card-hover hover:text-foreground',
                    )
                  }
                >
                  <Settings className="h-4 w-4" />
                </OverlayNavLink>
              )}
            </TooltipTrigger>
            <TooltipContent side="right">{t('navigation:settings')}</TooltipContent>
          </Tooltip>
        ) : desktop ? (
          <button
            type="button"
            onClick={() => void openSettingsWindow().catch(() => undefined)}
            className="flex w-full items-center gap-2.5 rounded-md px-3 py-1.5 text-sm text-muted-foreground transition-colors hover:bg-card-hover hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <Settings className="h-4 w-4" />
            {t('navigation:settings')}
          </button>
        ) : (
          <OverlayNavLink
            to="/settings"
            className={({ isActive }) =>
              cn(
                'flex items-center gap-2.5 rounded-md px-3 py-1.5 text-sm transition-colors',
                isActive
                  ? 'bg-primary/10 font-medium text-primary'
                  : 'text-muted-foreground hover:bg-card-hover hover:text-foreground',
              )
            }
          >
            <Settings className="h-4 w-4" />
            {t('navigation:settings')}
          </OverlayNavLink>
        )}
        <SidebarUserMenu collapsed={collapsed} />
      </div>
    </aside>
  )
}

/**
 * Sidebar affordance for the global Ctrl/Cmd+K palette. Discoverability is the
 * whole point — the chord alone hides the feature from anyone who has not been
 * told about it.
 */
function CommandTrigger({ collapsed }: { collapsed: boolean }) {
  const openPalette = useOpenCommandPalette()
  const { t } = useTranslation('navigation')
  const hint = commandShortcutHint()
  const button = (
    <button
      type="button"
      onClick={openPalette}
      aria-label={`${t('commandPalette.trigger')} (${hint})`}
      className={cn(
        'flex items-center rounded-md text-muted-foreground transition-colors hover:bg-card-hover hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
        collapsed ? 'h-8 w-8 justify-center' : 'w-full gap-2.5 px-3 py-1.5 text-sm',
      )}
    >
      <Command className="h-4 w-4 shrink-0" />
      {!collapsed ? (
        <>
          <span className="min-w-0 flex-1 truncate text-left">
            {t('commandPalette.trigger')}
          </span>
          <kbd className="rounded border border-border bg-muted px-1 py-px font-sans text-[10px] leading-4">
            {hint}
          </kbd>
        </>
      ) : null}
    </button>
  )
  if (!collapsed) return button
  return (
    <Tooltip>
      <TooltipTrigger asChild>{button}</TooltipTrigger>
      <TooltipContent side="right">
        {`${t('commandPalette.trigger')} · ${hint}`}
      </TooltipContent>
    </Tooltip>
  )
}

interface SidebarUserMenuProps {
  collapsed: boolean
}

function SidebarUserMenu({ collapsed }: SidebarUserMenuProps) {
  const { t } = useTranslation('navigation')
  const user = useAuthStore((s) => s.user)
  const logout = useAuthStore((s) => s.logout)
  const qc = useQueryClient()
  const navigate = useNavigate()
  const { closeAll } = useTerminalRuntime()
  const [open, setOpen] = useState(false)
  const [loggingOut, setLoggingOut] = useState(false)

  const onLogout = async () => {
    if (loggingOut) return
    setLoggingOut(true)
    try {
      await closeAll(true).catch(logTerminalCleanupError)
      logout()
      qc.clear()
      setOpen(false)
      void navigate('/login', { replace: true })
    } finally {
      setLoggingOut(false)
    }
  }

  if (!user) return null

  return (
    <div className={cn('relative', !collapsed && 'mt-1')}>
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className={cn(
          'flex items-center rounded-md transition-colors hover:bg-card-hover',
          collapsed
            ? 'h-8 w-8 justify-center'
            : 'w-full gap-2.5 px-2.5 py-1.5 text-left',
          open && 'bg-card-hover',
        )}
        aria-label={t('userMenu')}
      >
        <AgentAvatar name={user.name} kind="user" avatarUrl={user.avatar_url} size="sm" />
        {!collapsed && (
          <span className="min-w-0 flex-1 truncate text-sm font-medium">
            {user.name}
          </span>
        )}
      </button>
      {open && (
        <>
          <div
            className="fixed inset-0 z-10"
            onClick={() => setOpen(false)}
            aria-hidden
          />
          <div
            className={cn(
              'absolute z-20 w-56 rounded-lg border border-border bg-background p-3 shadow-lg',
              collapsed ? 'bottom-0 left-full ml-2' : 'bottom-full left-0 mb-2',
            )}
          >
            <div className="mb-2 border-b border-border pb-2">
              <p className="truncate text-sm font-medium">{user.name}</p>
              <p className="truncate text-xs text-muted-foreground">{user.email}</p>
            </div>
            <Button
              variant="ghost"
              size="sm"
              className="w-full justify-start gap-2"
              onClick={() => void onLogout()}
              disabled={loggingOut}
            >
              <LogOut className="h-4 w-4" />
              {t('logout')}
            </Button>
          </div>
        </>
      )}
    </div>
  )
}
