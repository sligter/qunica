import { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { NavLink, useNavigate } from 'react-router-dom'
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
} from 'lucide-react'

import { avatarColorClass } from '@/lib/avatarColor'
import { cn } from '@/lib/utils'
import { useGroups } from '@/hooks/useGroups'
import { useAuthStore } from '@/stores/authStore'
import { GroupFormDialog } from '@/components/groups/GroupFormDialog'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

const COLLAPSED_KEY = 'ag-swarmer:layout:sidebar-collapsed'

interface LibraryItem {
  to: string
  label: string
  icon: typeof Bot
}

const libraryItems: LibraryItem[] = [
  { to: '/agents', label: 'Agents', icon: Bot },
  { to: '/providers', label: 'Providers', icon: Plug },
  { to: '/skills', label: 'Skills', icon: Sparkles },
  { to: '/workspaces', label: 'Workspaces', icon: Folder },
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

function relativeTime(iso: string): string {
  const d = new Date(iso).getTime()
  const diff = Date.now() - d
  if (diff < 60_000) return 'now'
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m`
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h`
  if (diff < 7 * 86_400_000) return `${Math.floor(diff / 86_400_000)}d`
  return new Date(iso).toLocaleDateString()
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
  const [collapsed, setCollapsed] = useState(readCollapsed)
  const [dialogOpen, setDialogOpen] = useState(false)
  const [query, setQuery] = useState('')
  const groups = useGroups()

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

  return (
    <aside
      className={cn(
        'flex h-full shrink-0 flex-col border-r border-border bg-card',
        collapsed ? 'w-14' : 'w-[248px]',
      )}
    >
      <GroupFormDialog open={dialogOpen} onOpenChange={setDialogOpen} />

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
          aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
        >
          {collapsed ? (
            <PanelLeft className="h-4 w-4" />
          ) : (
            <PanelLeftClose className="h-4 w-4" />
          )}
        </Button>
      </div>

      {/* New group */}
      <div className={cn('shrink-0', collapsed ? 'flex justify-center pb-2' : 'px-3 pb-2')}>
        {collapsed ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                size="icon"
                onClick={() => setDialogOpen(true)}
                aria-label="New group"
              >
                <MessageSquarePlus className="h-4 w-4" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="right">New group</TooltipContent>
          </Tooltip>
        ) : (
          <Button
            className="w-full justify-start gap-2 rounded-lg"
            onClick={() => setDialogOpen(true)}
          >
            <MessageSquarePlus className="h-4 w-4" />
            New group
          </Button>
        )}
      </div>

      {/* Groups */}
      {collapsed ? (
        <div className="min-h-0 flex-1" />
      ) : (
        <div className="flex min-h-0 flex-1 flex-col">
          <div className="shrink-0 px-3 pt-2">
            <p className="px-1 pb-1.5 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
              Groups
            </p>
            <div className="relative">
              <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
              <Input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search groups"
                aria-label="Search groups"
                className="h-8 pl-8 text-xs"
              />
            </div>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-2 py-2">
            {groups.isLoading && (
              <p className="px-2 text-xs text-muted-foreground">Loading…</p>
            )}
            {groups.error && (
              <p className="px-2 text-xs text-destructive">Failed to load groups.</p>
            )}
            {groups.data && groups.data.length === 0 && (
              <p className="px-2 text-xs text-muted-foreground">
                No groups yet. Click New group to start one.
              </p>
            )}
            {groups.data && groups.data.length > 0 && filteredGroups.length === 0 && (
              <p className="px-2 text-xs text-muted-foreground">No matches.</p>
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
                          {relativeTime(g.created_at)}
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
            Library
          </p>
        )}
        {libraryItems.map(({ to, label, icon: Icon }) =>
          collapsed ? (
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
          ),
        )}
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
                aria-label="Settings"
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
            <TooltipContent side="right">Settings</TooltipContent>
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
            Settings
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
  const user = useAuthStore((s) => s.user)
  const logout = useAuthStore((s) => s.logout)
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [open, setOpen] = useState(false)

  const onLogout = () => {
    logout()
    qc.clear()
    setOpen(false)
    void navigate('/login', { replace: true })
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
        aria-label="User menu"
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
              onClick={onLogout}
            >
              <LogOut className="h-4 w-4" />
              Logout
            </Button>
          </div>
        </>
      )}
    </div>
  )
}
