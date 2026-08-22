import { Suspense, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, Outlet, useLocation } from 'react-router-dom'
import { ArrowLeft, Library } from 'lucide-react'

import { RouteFallback } from '@/components/layout/RouteFallback'
import { ResourceRail } from '@/components/layout/ResourceRail'
import { useCloseOverlay } from '@/components/layout/overlayRouting'
import { Button } from '@/components/ui/button'
import { useDocumentTitle } from '@/components/ui/section'
import { cn } from '@/lib/utils'

type EntityArea =
  | 'agents'
  | 'providers'
  | 'mcpServers'
  | 'skills'
  | 'workspaces'
  | 'usage'

interface EntityLayoutProps {
  /** Navigation key for the area, used for the document title. */
  titleKey: EntityArea
  /**
   * The searchable entity list column rendered left of the detail Outlet.
   * Omitted for areas that are a single full-width report rather than a
   * collection — Token usage has nothing to list.
   */
  list?: ReactNode
}

/**
 * Standalone shell for the library: a header naming the surface, the persistent
 * resource rail, the entity list column, and the detail/create Outlet.
 *
 * The header names the *library*, not the area. Which area you are in is the
 * rail's job (it is the only element that also says where else you could go),
 * and the list column repeats it over the rows it holds. Naming the area a
 * third time up here is what put three copies of the same word — and, before
 * this, three copies of the same "New" button — on one screen.
 */
export function EntityLayout({ titleKey, list }: EntityLayoutProps) {
  const { t } = useTranslation(['navigation', 'common'])
  const location = useLocation()
  const close = useCloseOverlay()
  const area = t(titleKey)
  const basePath = titleKey === 'mcpServers' ? '/mcp-servers' : `/${titleKey}`
  const detailOpen = Boolean(list) && location.pathname.replace(/\/$/, '') !== basePath

  // The shared hook adds the product suffix and restores the previous title
  // on unmount — the raw assignment here never did the second half.
  useDocumentTitle(area)

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
        <span
          aria-hidden
          className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary"
        >
          <Library className="h-4 w-4" />
        </span>
        <h1 className="min-w-0 truncate font-serif text-base font-semibold tracking-tight">
          {t('library')}
        </h1>
      </div>
      {/* The rail stacks above the panes below `lg`, where 56px of it beside a
          list and a detail view would leave the detail unusable. */}
      <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden lg:flex-row">
        <ResourceRail />
        <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden">
          {list ? (
            <div
              data-slot="entity-list-pane"
              className={cn(
                'min-h-0 min-w-0 shrink-0 lg:h-full',
                detailOpen ? 'max-lg:hidden' : 'max-lg:flex max-lg:w-full max-lg:flex-1',
              )}
            >
              {list}
            </div>
          ) : null}
          <div
            data-slot="entity-detail-pane"
            className={cn(
              'flex min-w-0 flex-1 flex-col overflow-hidden',
              list && !detailOpen && 'max-lg:hidden',
            )}
          >
            {list && detailOpen ? (
              <div className="shrink-0 border-b border-border px-3 py-1.5 lg:hidden">
                <Button variant="ghost" size="sm" className="gap-1.5" asChild>
                  <Link to={basePath}>
                    <ArrowLeft className="h-3.5 w-3.5" />
                    {t('common:actions.backToList')}
                  </Link>
                </Button>
              </div>
            ) : null}
            {/* Keeps the list column painted while the detail chunk downloads. */}
            <Suspense fallback={<RouteFallback />}>
              <Outlet key={location.pathname} />
            </Suspense>
          </div>
        </div>
      </div>
    </div>
  )
}
