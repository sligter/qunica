import type { ReactNode } from 'react'
import { NavLink, Outlet, useNavigate } from 'react-router-dom'
import { ArrowLeft, Bot, Folder, Plug, SlidersHorizontal, Sparkles } from 'lucide-react'

import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'

interface SettingsNavItem {
  to: string
  label: string
  icon: typeof SlidersHorizontal
  end?: boolean
}

const navItems: SettingsNavItem[] = [
  { to: '/settings/general', label: 'General', icon: SlidersHorizontal },
  { to: '/settings/agents', label: 'Agents', icon: Bot },
  { to: '/settings/providers', label: 'Providers', icon: Plug },
  { to: '/settings/skills', label: 'Skills', icon: Sparkles },
  { to: '/settings/workspaces', label: 'Workspaces', icon: Folder },
]

/**
 * Claude-style settings surface: a fixed left navigation column (General plus
 * the entity areas) and a content Outlet on the right. Entity areas nest their
 * own list + detail columns via SettingsEntityLayout.
 */
export function SettingsLayout() {
  const navigate = useNavigate()

  return (
    <div className="flex h-full w-full overflow-hidden bg-background">
      <nav className="flex w-[220px] shrink-0 flex-col border-r border-border bg-card">
        <div className="flex h-14 shrink-0 items-center gap-2 px-3">
          <Button
            variant="ghost"
            size="icon"
            onClick={() => void navigate('/')}
            aria-label="Back to chat"
          >
            <ArrowLeft className="h-4 w-4" />
          </Button>
          <h1 className="font-serif text-base font-semibold tracking-tight">
            Settings
          </h1>
        </div>
        <ul className="flex-1 space-y-0.5 overflow-y-auto px-2 py-2">
          {navItems.map(({ to, label, icon: Icon, end }) => (
            <li key={to}>
              <NavLink
                to={to}
                end={end}
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
            </li>
          ))}
        </ul>
      </nav>
      <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
        <Outlet />
      </div>
    </div>
  )
}

interface SettingsEntityLayoutProps {
  /** The searchable entity list column rendered left of the detail Outlet. */
  list: ReactNode
}

/**
 * Nested layout for entity areas inside the settings surface:
 * a fixed-width list column plus the detail/create Outlet.
 */
export function SettingsEntityLayout({ list }: SettingsEntityLayoutProps) {
  return (
    <div className="flex h-full min-w-0 flex-1 overflow-hidden">
      {list}
      <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
        <Outlet />
      </div>
    </div>
  )
}
