import { NavLink } from 'react-router-dom'
import { Bot, MessagesSquare, Plug, Settings, Sparkles } from 'lucide-react'

import { RailUserMenu } from '@/components/layout/RailUserMenu'
import { cn } from '@/lib/utils'

interface RailItem {
  to: string
  label: string
  icon: typeof MessagesSquare
}

const items: RailItem[] = [
  { to: '/groups', label: 'Groups', icon: MessagesSquare },
  { to: '/agents', label: 'Agents', icon: Bot },
  { to: '/providers', label: 'Providers', icon: Plug },
  { to: '/skills', label: 'Skills', icon: Sparkles },
  { to: '/settings/system', label: 'Settings', icon: Settings },
]

export function SidebarRail() {
  return (
    <aside className="flex w-16 shrink-0 flex-col items-stretch border-r border-border bg-card">
      <div className="flex h-14 items-center justify-center border-b border-border">
        <div
          className="flex h-9 w-9 items-center justify-center rounded-lg bg-primary text-sm font-semibold text-primary-foreground"
          title="AgentChat"
        >
          AC
        </div>
      </div>
      <nav className="flex flex-1 flex-col gap-1 p-2">
        {items.map(({ to, label, icon: Icon }) => (
          <NavLink
            key={to}
            to={to}
            className={({ isActive }) =>
              cn(
                'group relative flex flex-col items-center gap-0.5 rounded-md px-1 py-2 text-[10px] font-medium transition-colors',
                isActive
                  ? 'bg-primary/10 text-primary before:absolute before:left-0 before:top-1.5 before:bottom-1.5 before:w-0.5 before:rounded-r-full before:bg-primary'
                  : 'text-muted-foreground hover:bg-card-hover hover:text-foreground',
              )
            }
            title={label}
          >
            <Icon className="h-5 w-5" />
            <span>{label}</span>
          </NavLink>
        ))}
      </nav>
      <div className="border-t border-border p-2">
        <RailUserMenu />
      </div>
    </aside>
  )
}
