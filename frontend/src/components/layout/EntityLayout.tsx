import { Suspense, useEffect, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, Outlet, useLocation } from 'react-router-dom'
import { ArrowLeft, Bot, Folder, Plug, Plus, Server, Sparkles } from 'lucide-react'

import { RouteFallback } from '@/components/layout/RouteFallback'
import { useCloseOverlay } from '@/components/layout/overlayRouting'
import { Button } from '@/components/ui/button'

type EntityArea = 'agents' | 'providers' | 'mcpServers' | 'skills' | 'workspaces'

/**
 * Per-area presentation metadata: glyph, route root, and where the "new"
 * action points. Kept here rather than threaded through props so every entity
 * area inherits the same header treatment for free.
 */
const AREA_META: Record<
  EntityArea,
  { icon: typeof Bot; path: string; newNs: string; newKey: string }
> = {
  agents: { icon: Bot, path: '/agents', newNs: 'agents', newKey: 'new' },
  providers: { icon: Plug, path: '/providers', newNs: 'providers', newKey: 'new' },
  mcpServers: { icon: Server, path: '/mcp-servers', newNs: 'mcp', newKey: 'new' },
  skills: { icon: Sparkles, path: '/skills', newNs: 'skills', newKey: 'import' },
  workspaces: { icon: Folder, path: '/workspaces', newNs: 'workspaces', newKey: 'new' },
}

interface EntityLayoutProps {
  /** Navigation resource key for the area heading. */
  titleKey: EntityArea
  /** The searchable entity list column rendered left of the detail Outlet. */
  list: ReactNode
}

/**
 * Standalone shell for a top-level library area (Agents, Providers, MCP,
 * Skills, Workspaces): a header with back-to-chat, an icon chip over the serif
 * area title with a one-line description, and a primary "new" action — above
 * the entity list column and the detail/create Outlet.
 *
 * The list column carries the same area icon so loading skeletons, empty
 * states, and this header all read as one system.
 */
export function EntityLayout({ titleKey, list }: EntityLayoutProps) {
  const { t } = useTranslation('navigation')
  const location = useLocation()
  const close = useCloseOverlay()
  const meta = AREA_META[titleKey]
  const Icon = meta.icon
  const title = t(titleKey)
  const description = t(`libraryDescriptions.${titleKey}`, { defaultValue: '' })
  // The create form is lazy-loaded; pointing straight at its route lets React
  // Router start fetching it while the click is still settling.
  const newTo = `${meta.path}/new`
  const newLabel = t(`${meta.newNs}:${meta.newKey}`)

  useEffect(() => {
    document.title = title
  }, [title])

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
            <Icon className="h-4 w-4" />
          </span>
          <div className="min-w-0 leading-tight">
            <h1 className="truncate font-serif text-base font-semibold tracking-tight">
              {title}
            </h1>
            {description ? (
              <p className="truncate text-xs text-muted-foreground">{description}</p>
            ) : null}
          </div>
        </div>
        <Button size="sm" className="ml-auto gap-1 shadow-xs" asChild>
          <Link to={newTo}>
            <Plus className="h-3.5 w-3.5" />
            {newLabel}
          </Link>
        </Button>
      </div>
      <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden">
        {list}
        <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
          {/* Keeps the list column painted while the detail chunk downloads. */}
          <Suspense fallback={<RouteFallback />}>
            <Outlet key={location.pathname} />
          </Suspense>
        </div>
      </div>
    </div>
  )
}
