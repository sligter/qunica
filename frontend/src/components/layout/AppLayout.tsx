import { useQueryClient } from '@tanstack/react-query'
import { Link, NavLink, Outlet, useNavigate } from 'react-router-dom'

import { Button } from '@/components/ui/button'
import { useAuthStore } from '@/stores/authStore'
import { cn } from '@/lib/utils'

const navItemClass = ({ isActive }: { isActive: boolean }) =>
  cn(
    'rounded-md px-3 py-1.5 text-sm font-medium transition-colors',
    isActive
      ? 'bg-muted text-foreground'
      : 'text-muted-foreground hover:bg-muted hover:text-foreground',
  )

export function AppLayout() {
  const user = useAuthStore((s) => s.user)
  const logout = useAuthStore((s) => s.logout)
  const qc = useQueryClient()
  const navigate = useNavigate()

  const onLogout = () => {
    logout()
    qc.clear()
    void navigate('/login', { replace: true })
  }

  return (
    <div className="flex h-screen flex-col bg-background">
      <header className="flex h-13 items-center justify-between border-b border-border px-4 py-2">
        <div className="flex items-center gap-6">
          <Link to="/groups" className="text-base font-semibold tracking-tight">
            AgentChat
          </Link>
          <nav className="flex items-center gap-1">
            <NavLink to="/groups" className={navItemClass}>
              Groups
            </NavLink>
            <NavLink to="/agents" className={navItemClass}>
              Agents
            </NavLink>
          </nav>
        </div>
        <div className="flex items-center gap-3">
          {user && (
            <span className="text-sm text-muted-foreground">{user.name}</span>
          )}
          <Button variant="outline" size="sm" onClick={onLogout}>
            Logout
          </Button>
        </div>
      </header>
      <main className="flex-1 overflow-hidden">
        <Outlet />
      </main>
    </div>
  )
}
