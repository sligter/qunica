import { Suspense } from 'react'
import { NavLink, Outlet, useNavigate } from 'react-router-dom'
import { ArrowLeft, ScrollText, Sparkles, SlidersHorizontal } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { RouteFallback } from '@/components/layout/RouteFallback'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

/** Global settings shell with a compact system/logs navigation. */
export function SettingsLayout() {
  const { t } = useTranslation(['navigation', 'settings', 'assistant'])
  const navigate = useNavigate()

  return (
    <div className="flex h-full w-full flex-col overflow-hidden bg-background">
      <div className="flex h-14 shrink-0 items-center gap-2 border-b border-border px-3">
        <Button
          variant="ghost"
          size="icon"
          onClick={() => void navigate('/')}
          aria-label={t('backToChat')}
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <h1 className="font-serif text-base font-semibold tracking-tight">
          {t('settings')}
        </h1>
      </div>
      <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden md:flex-row">
        <aside className="shrink-0 border-b border-border bg-card p-2 md:w-52 md:border-b-0 md:border-r md:p-3">
          <p className="hidden px-3 pb-2 pt-1 text-xs font-medium uppercase tracking-wider text-muted-foreground md:block">
            {t('settings:tabs.preferences')}
          </p>
          <nav className="flex gap-1 md:flex-col" aria-label={t('navigation:settings')}>
            {[
              { to: '/settings/system', label: t('settings:tabs.system'), icon: SlidersHorizontal },
              { to: '/settings/logs', label: t('settings:tabs.logs'), icon: ScrollText },
              {
                to: '/settings/assistant-actions',
                label: t('assistant:actions.title'),
                icon: Sparkles,
              },
            ].map(({ to, label, icon: Icon }) => (
              <NavLink
                key={to}
                to={to}
                className={({ isActive }) =>
                  cn(
                    'flex items-center gap-2.5 rounded-md px-3 py-2 text-sm transition-colors',
                    isActive
                      ? 'bg-primary/10 font-medium text-primary'
                      : 'text-muted-foreground hover:bg-card-hover hover:text-foreground',
                  )
                }
              >
                <Icon className="h-4 w-4" />
                {label}
              </NavLink>
            ))}
          </nav>
        </aside>
        <div className="min-h-0 min-w-0 flex-1 overflow-hidden">
          <Suspense fallback={<RouteFallback />}>
            <Outlet />
          </Suspense>
        </div>
      </div>
    </div>
  )
}
