import { Suspense } from 'react'
import { NavLink, Outlet } from 'react-router-dom'
import { ArrowLeft, Images, ScrollText, Settings, Sparkles, SlidersHorizontal } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { RouteFallback } from '@/components/layout/RouteFallback'
import { useCloseOverlay } from '@/components/layout/overlayRouting'
import { Button } from '@/components/ui/button'
import { useDocumentTitle } from '@/components/ui/section'
import { navItemClass } from '@/lib/navItemClass'

/** Global settings shell with a compact system/logs navigation. */
export function SettingsLayout() {
  const { t } = useTranslation(['navigation', 'settings', 'assistant'])
  const close = useCloseOverlay()
  useDocumentTitle(t('settings'))
  const groups = [
    {
      label: t('settings:groups.preferences'),
      items: [
        { to: '/settings/system', label: t('settings:tabs.system'), icon: SlidersHorizontal },
        { to: '/settings/media', label: t('settings:tabs.media'), icon: Images },
      ],
    },
    {
      label: t('settings:groups.diagnostics'),
      items: [
        { to: '/settings/logs', label: t('settings:tabs.logs'), icon: ScrollText },
        {
          to: '/settings/assistant-actions',
          label: t('assistant:actions.title'),
          icon: Sparkles,
        },
      ],
    },
  ]

  return (
    <div className="flex h-full w-full flex-col overflow-hidden bg-background">
      <div className="flex h-14 shrink-0 items-center gap-2 border-b border-border px-3">
        <Button
          variant="ghost"
          size="icon"
          onClick={close}
          aria-label={t('backToChat')}
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <div className="flex min-w-0 items-center gap-2.5">
          <span
            aria-hidden
            className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary"
          >
            <Settings className="h-4 w-4" />
          </span>
          <div className="min-w-0 leading-tight">
            <h1 className="truncate font-serif text-base font-semibold tracking-tight">
              {t('settings')}
            </h1>
            <p className="truncate text-xs text-muted-foreground">
              {t('settings:description')}
            </p>
          </div>
        </div>
      </div>
      <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden md:flex-row">
        <aside className="shrink-0 overflow-hidden border-b border-border bg-card p-2 md:w-52 md:border-b-0 md:border-r md:p-3">
          <nav
            className="flex gap-3 overflow-x-auto overscroll-x-contain md:flex-col md:gap-5 md:overflow-visible"
            aria-label={t('navigation:settings')}
          >
            {groups.map((group) => (
              <div
                key={group.label}
                role="group"
                aria-label={group.label}
                className="flex shrink-0 gap-1 md:w-full md:flex-col"
              >
                <p
                  aria-hidden
                  className="hidden px-3 pb-1 pt-1 text-xs font-medium uppercase tracking-wider text-muted-foreground md:block"
                >
                  {group.label}
                </p>
                {group.items.map(({ to, label, icon: Icon }) => (
                  <NavLink
                    key={to}
                    to={to}
                    className={({ isActive }) =>
                      navItemClass(
                        isActive,
                        'w-auto shrink-0 items-center gap-2.5 px-3 py-2 text-sm md:w-full',
                      )
                    }
                  >
                    <Icon className="h-4 w-4 shrink-0 text-muted-foreground" />
                    {label}
                  </NavLink>
                ))}
              </div>
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
