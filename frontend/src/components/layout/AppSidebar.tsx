import { lazy, Suspense, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { NavLink, useLocation, useNavigate } from 'react-router-dom'
import {
  Bot,
  Folder,
  LogOut,
  MessageSquarePlus,
  PanelLeft,
  PanelLeftClose,
  Plug,
  Search,
  Settings,
  Sparkles,
  Trash2,
} from 'lucide-react'

import { avatarColorClass } from '@/lib/avatarColor'
import { formatRelativeTime } from '@/lib/format'
import { cn } from '@/lib/utils'
import { normalizeLanguage } from '@/i18n'
import { useGroups } from '@/hooks/useGroups'
import { useDeleteDirectChat, useDirectChats } from '@/hooks/useDirectChats'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { useAuthStore } from '@/stores/authStore'
import { DirectChatPickerDialog } from '@/components/direct-chats/DirectChatPickerDialog'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { logTerminalCleanupError } from '@/terminal/logTerminalCleanupError'
import { useTerminalRuntime } from '@/terminal/TerminalRuntimeProvider'

const COLLAPSED_KEY = 'ag-swarmer:layout:sidebar-collapsed'

// The create-group form drags in react-hook-form + zod; it downloads the first
// time the user asks for it rather than on every app boot.
const GroupFormDialog = lazy(() =>
  import('@/components/groups/GroupFormDialog').then((m) => ({ default: m.GroupFormDialog })),
)

interface LibraryItem {
  to: string
  key: 'agents' | 'providers' | 'skills' | 'workspaces'
  icon: typeof Bot
}

const libraryItems: LibraryItem[] = [
  { to: '/agents', key: 'agents', icon: Bot },
  { to: '/providers', key: 'providers', icon: Plug },
  { to: '/skills', key: 'skills', icon: Sparkles },
  { to: '/workspaces', key: 'workspaces', icon: Folder },
]

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

function initials(name: string | undefined): string {
  if (!name) return '?'
  const parts = name.trim().split(/\s+/)
  if (parts.length === 0) return '?'
  if (parts.length === 1) return parts[0]!.slice(0, 2).toUpperCase()
  return (parts[0]![0]! + parts[parts.length - 1]![0]!).toUpperCase()
}

/**
 * Unified app sidebar: New group, searchable group list, Library links into the
 * settings surface, and a bottom Settings row plus user menu. Collapses to a
 * narrow icon strip; the collapsed state persists in localStorage.
 */
export function AppSidebar() {
  const { t, i18n } = useTranslation(['navigation', 'groups', 'common'])
  const [collapsed, setCollapsed] = useState(readCollapsed)
  const [dialogOpen, setDialogOpen] = useState(false)
  const [directDialogOpen, setDirectDialogOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [pendingDeleteChat, setPendingDeleteChat] = useState<{ id: string; title: string } | null>(null)
  const navigate = useNavigate()
  const location = useLocation()
  const groups = useGroups()
  const directChats = useDirectChats()
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

  return (
    <aside
      className={cn(
        'flex h-full shrink-0 flex-col border-r border-border bg-card',
        collapsed ? 'w-14' : 'w-[248px]',
      )}
    >
      {dialogOpen ? (
        <Suspense fallback={null}>
          <GroupFormDialog open onOpenChange={setDialogOpen} />
        </Suspense>
      ) : null}
      <DirectChatPickerDialog open={directDialogOpen} onOpenChange={setDirectDialogOpen} />
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
            <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-primary font-serif text-sm font-semibold text-primary-foreground">
              A
            </div>
            <span className="min-w-0 flex-1 truncate font-serif text-sm font-semibold tracking-tight">
              AG Swarmer
            </span>
          </>
        )}
        <Button
          variant="ghost"
          size="icon"
          onClick={toggleCollapsed}
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
          <div className="shrink-0 px-3 pt-2">
            <p className="px-1 pb-1.5 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
              {t('navigation:searchConversations')}
            </p>
            <div className="relative">
              <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
              <Input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t('navigation:searchConversations')}
                aria-label={t('navigation:searchConversations')}
                className="h-8 pl-8 text-xs"
              />
            </div>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-2 py-2">
            <p className="px-1 pb-1.5 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
              {t('navigation:directChats')}
            </p>
            {directChats.isLoading ? (
              <p className="px-2 pb-2 text-xs text-muted-foreground">{t('common:state.loading')}</p>
            ) : null}
            {directChats.error ? (
              <p className="px-2 pb-2 text-xs text-destructive">{String(directChats.error)}</p>
            ) : null}
            <ul className="mb-3 space-y-0.5">
              {filteredDirectChats.map((chat) => (
                <li key={chat.id} className="group flex items-center gap-0.5">
                  <NavLink
                    to={`/chats/${chat.id}`}
                    className={({ isActive }) =>
                      cn(
                        'flex min-w-0 flex-1 items-center gap-2.5 rounded-md px-2.5 py-2 text-left transition-colors',
                        isActive ? 'bg-primary/10' : 'hover:bg-card-hover',
                      )
                    }
                  >
                    {({ isActive }) => (
                      <>
                        <Avatar className="h-7 w-7 shrink-0">
                          <AvatarFallback className={avatarColorClass(chat.id)}>
                            {(chat.agent_name ?? chat.title).slice(0, 1).toUpperCase()}
                          </AvatarFallback>
                        </Avatar>
                        <span
                          className={cn(
                            'min-w-0 flex-1 truncate text-sm',
                            isActive ? 'font-semibold' : 'font-medium',
                          )}
                        >
                          {chat.title}
                        </span>
                        <span className="shrink-0 text-[10px] text-muted-foreground">
                          {formatRelativeTime(
                            chat.updated_at,
                            normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US',
                          )}
                        </span>
                      </>
                    )}
                  </NavLink>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        className="h-7 w-7 shrink-0 opacity-0 transition-opacity hover:text-destructive focus:opacity-100 group-hover:opacity-100"
                        aria-label={t('chat:direct.deleteNamed', { title: chat.title })}
                        onClick={() => setPendingDeleteChat({ id: chat.id, title: chat.title })}
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>{t('common:actions.delete')}</TooltipContent>
                  </Tooltip>
                </li>
              ))}
            </ul>
            <p className="px-1 pb-1.5 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
              {t('navigation:groups')}
            </p>
            {groups.isLoading && (
              <p className="px-2 text-xs text-muted-foreground">{t('common:state.loading')}</p>
            )}
            {groups.error && (
              <p className="px-2 text-xs text-destructive">{t('groups:loadError')}</p>
            )}
            {groups.data && groups.data.length === 0 && (
              <p className="px-2 text-xs text-muted-foreground">
                {t('groups:empty')}
              </p>
            )}
            {groups.data && groups.data.length > 0 && filteredGroups.length === 0 && (
              <p className="px-2 text-xs text-muted-foreground">{t('common:state.noMatches')}</p>
            )}
            <ul className="space-y-0.5">
              {filteredGroups.map((g) => (
                <li key={g.id}>
                  <NavLink
                    to={`/groups/${g.id}`}
                    className={({ isActive }) =>
                      cn(
                        'flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left transition-colors',
                        isActive ? 'bg-primary/10' : 'hover:bg-card-hover',
                      )
                    }
                  >
                    {({ isActive }) => (
                      <>
                        <Avatar className="h-7 w-7 shrink-0">
                          <AvatarFallback className={avatarColorClass(g.id)}>
                            {g.name.slice(0, 1).toUpperCase()}
                          </AvatarFallback>
                        </Avatar>
                        <span
                          className={cn(
                            'min-w-0 flex-1 truncate text-sm',
                            isActive ? 'font-semibold' : 'font-medium',
                          )}
                        >
                          {g.name}
                        </span>
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
            </ul>
          </div>
        </div>
      )}

      {/* Library */}
      <nav
        className={cn(
          'shrink-0 border-t border-border py-2',
          collapsed ? 'flex flex-col items-center gap-1' : 'px-2',
        )}
      >
        {!collapsed && (
          <p className="px-3 pb-1.5 pt-1 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
            {t('navigation:library')}
          </p>
        )}
        {libraryItems.map(({ to, key, icon: Icon }) => {
          const label = t(`navigation:${key}`)
          return collapsed ? (
            <Tooltip key={to}>
              <TooltipTrigger asChild>
                <NavLink
                  to={to}
                  aria-label={label}
                  className={({ isActive }) =>
                    cn(
                      'flex h-8 w-8 items-center justify-center rounded-md transition-colors',
                      isActive
                        ? 'bg-primary/10 text-primary'
                        : 'text-muted-foreground hover:bg-card-hover hover:text-foreground',
                    )
                  }
                >
                  <Icon className="h-4 w-4" />
                </NavLink>
              </TooltipTrigger>
              <TooltipContent side="right">{label}</TooltipContent>
            </Tooltip>
          ) : (
            <NavLink
              key={to}
              to={to}
              className={({ isActive }) =>
                cn(
                  'flex items-center gap-2.5 rounded-md px-3 py-1.5 text-sm transition-colors',
                  isActive
                    ? 'bg-primary/10 font-medium text-primary'
                    : 'text-muted-foreground hover:bg-card-hover hover:text-foreground',
                )
              }
            >
              <Icon className="h-4 w-4" />
              {label}
            </NavLink>
          )
        })}
      </nav>

      {/* Bottom: Settings + user menu */}
      <div
        className={cn(
          'shrink-0 border-t border-border py-2',
          collapsed ? 'flex flex-col items-center gap-1' : 'px-2',
        )}
      >
        {collapsed ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <NavLink
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
              </NavLink>
            </TooltipTrigger>
            <TooltipContent side="right">{t('navigation:settings')}</TooltipContent>
          </Tooltip>
        ) : (
          <NavLink
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
          </NavLink>
        )}
        <SidebarUserMenu collapsed={collapsed} />
      </div>
    </aside>
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
        <Avatar className="h-7 w-7 shrink-0">
          <AvatarFallback className="bg-primary text-xs text-primary-foreground">
            {initials(user.name)}
          </AvatarFallback>
        </Avatar>
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
