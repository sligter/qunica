import { Suspense } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, Outlet, useLocation } from 'react-router-dom'
import { ArrowLeft, ChevronRight, Library } from 'lucide-react'

import { RouteFallback } from '@/components/layout/RouteFallback'
import { ResourceRail } from '@/components/layout/ResourceRail'
import { useCloseOverlay } from '@/components/layout/overlayRouting'
import { Button } from '@/components/ui/button'
import { useDocumentTitle } from '@/components/ui/section'
import { isAuxiliaryDesktopWindow } from '@/lib/desktop'
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
}

/**
 * Standalone shell for the library: a header naming the surface, the persistent
 * resource rail, and the full-width detail/create Outlet.
 *
 * The header names the *library*, not the area. Which area you are in is the
 * rail's job — and once a detail or create page is open, that area name is
 * also the way back: with the list column gone there is no other affordance
 * pointing at the index, so the breadcrumb carries it instead.
 */
export function EntityLayout({ titleKey }: EntityLayoutProps) {
  const { t } = useTranslation(['navigation', 'common'])
  const location = useLocation()
  const close = useCloseOverlay()
  const auxiliaryWindow = isAuxiliaryDesktopWindow()
  const area = t(titleKey)
  const basePath = titleKey === 'mcpServers' ? '/mcp-servers' : `/${titleKey}`
  // A detail or create page is open when the path goes deeper than the area
  // root (/skills/new, /agents/:id). On the root itself there is nothing to
  // navigate back to, so the area renders as plain text there.
  const detailOpen = location.pathname.replace(/\/$/, '') !== basePath

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
          aria-label={t(auxiliaryWindow ? 'closeWindow' : 'backToChat')}
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <span
          aria-hidden
          className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary"
        >
          <Library className="h-4 w-4" />
        </span>
        {/* Breadcrumb: on the index the header just names the area; once a
            detail or create page is open, that name becomes the link back to
            the index. With the list column gone this is the only affordance
            pointing at the grid. */}
        <h1 className="flex min-w-0 items-center gap-1 truncate font-serif text-base font-semibold tracking-tight">
          <Link
            to={basePath}
            className={cn(
              'shrink-0 transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
              detailOpen ? 'text-muted-foreground' : 'text-foreground',
            )}
          >
            {area}
          </Link>
          {detailOpen ? (
            <ChevronRight aria-hidden className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
          ) : null}
        </h1>
      </div>
      {/* The rail stacks above the pane below `lg`, where 56px of it beside a
          detail view would leave the content unusable. */}
      <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden lg:flex-row">
        <ResourceRail />
        <div className="min-h-0 min-w-0 flex-1 overflow-hidden">
          <div data-slot="entity-detail-pane" className="flex h-full min-w-0 flex-col overflow-hidden">
            {/* Keeps the previous page painted while the next chunk downloads. */}
            <Suspense fallback={<RouteFallback />}>
              <Outlet key={location.pathname} />
            </Suspense>
          </div>
        </div>
      </div>
    </div>
  )
}
