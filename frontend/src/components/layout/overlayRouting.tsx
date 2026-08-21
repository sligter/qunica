/**
 * Routing plumbing for the settings overlay.
 *
 * Settings, the resource areas and group management are still ordinary routes —
 * they have URLs, they deep-link, they survive a reload. What changed is where
 * they render: instead of replacing the conversation through `AppLayout`'s
 * `<Outlet />`, they float over it while the chat surface underneath stays
 * mounted, so a draft, a scroll position and an open workspace panel are still
 * there when the panel closes.
 *
 * The mechanism is React Router's background-location pattern. A link into an
 * overlay area carries the conversation it was opened from in
 * `location.state.backgroundLocation`; `AppLayout` renders that location as the
 * stage and the matched route as the overlay. Open one of these URLs cold — a
 * reload, a bookmark — and there is no background location, so the same route
 * renders as a plain full-height page instead.
 *
 * `navigate()` calls made *inside* an overlay (deleting an agent bounces to the
 * list, saving a new provider bounces to its detail page) do not carry the
 * state forward. Rather than thread it through a dozen call sites, `AppLayout`
 * remembers the last conversation it rendered and falls back to it, so those
 * hops keep the same stage. The state is what survives a history
 * back/forward; the remembered location is what survives a plain `navigate`.
 */

import { createContext, useCallback, useContext, useMemo } from 'react'
import {
  Link,
  NavLink,
  Navigate,
  useLocation,
  useNavigate,
  type Location,
  type NavLinkProps,
  type LinkProps,
} from 'react-router-dom'

/** Top-level areas that render as an overlay over the conversation stage. */
const OVERLAY_PREFIXES = [
  '/settings',
  '/agents',
  '/providers',
  '/mcp-servers',
  '/skills',
  '/workspaces',
  '/usage',
]

const GROUP_MANAGE = /^\/groups\/[^/]+\/manage\/?$/

/** Whether a pathname belongs to a settings-class area rather than the stage. */
export function isOverlayPath(pathname: string): boolean {
  if (GROUP_MANAGE.test(pathname)) return true
  return OVERLAY_PREFIXES.some(
    (prefix) => pathname === prefix || pathname.startsWith(`${prefix}/`),
  )
}

/**
 * Translation key naming the area, for the panel's accessible name. The panel
 * is a dialog, and "dialog" on its own tells a screen reader nothing about
 * which of seven areas just opened.
 */
export function overlayAreaLabelKey(pathname: string): string {
  if (GROUP_MANAGE.test(pathname)) return 'groups:manage.title'
  const prefix = OVERLAY_PREFIXES.find(
    (candidate) => pathname === candidate || pathname.startsWith(`${candidate}/`),
  )
  switch (prefix) {
    case '/agents':
      return 'navigation:agents'
    case '/providers':
      return 'navigation:providers'
    case '/mcp-servers':
      return 'navigation:mcpServers'
    case '/skills':
      return 'navigation:skills'
    case '/workspaces':
      return 'navigation:workspaces'
    case '/usage':
      return 'navigation:usage'
    default:
      return 'navigation:settings'
  }
}

export interface OverlayLocationState {
  /** The conversation the overlay was opened from, if it was opened from one. */
  backgroundLocation?: Location
}

/** What a link needs to carry to open (or stay inside) the overlay. */
export function overlayLinkState(location: Location): OverlayLocationState | undefined {
  const carried = (location.state as OverlayLocationState | null)?.backgroundLocation
  if (carried) return { backgroundLocation: carried }
  // Already inside the overlay with nothing carried: this is a cold deep link
  // rendering full-page, and it should stay that way rather than suddenly
  // acquiring a stage made of itself.
  if (isOverlayPath(location.pathname)) return undefined
  return { backgroundLocation: location }
}

/** The link state for opening an overlay area from wherever the user is now. */
export function useOverlayLinkState(): OverlayLocationState | undefined {
  const location = useLocation()
  return useMemo(() => overlayLinkState(location), [location])
}

interface OverlayContextValue {
  /** Conversation location under the overlay; null when it renders full-page. */
  background: Location | null
}

const OverlayContext = createContext<OverlayContextValue>({ background: null })

export const OverlayProvider = OverlayContext.Provider

/** True while the current route is rendering as a panel over a conversation. */
export function useIsOverlayModal(): boolean {
  return useContext(OverlayContext).background !== null
}

/**
 * Closes the overlay: back to the conversation it was opened over, or to
 * `fallback` when it was deep-linked and there is nothing behind it.
 */
export function useCloseOverlay(fallback = '/'): () => void {
  const { background } = useContext(OverlayContext)
  const navigate = useNavigate()
  return useCallback(() => {
    if (!background) {
      void navigate(fallback)
      return
    }
    void navigate({
      pathname: background.pathname,
      search: background.search,
      hash: background.hash,
    })
  }, [background, fallback, navigate])
}

/** `Link` that opens the overlay without losing the conversation behind it. */
export function OverlayLink({ ...props }: LinkProps) {
  const state = useOverlayLinkState()
  return <Link {...props} state={state} />
}

/** `NavLink` counterpart of {@link OverlayLink}. */
export function OverlayNavLink({ ...props }: NavLinkProps) {
  const state = useOverlayLinkState()
  return <NavLink {...props} state={state} />
}

/**
 * `Navigate` that keeps the background location across a redirect. The legacy
 * `/settings/<area>` deep links all funnel through here, so following one from
 * a conversation still lands in the overlay rather than dropping to full-page.
 */
export function OverlayRedirect({ to }: { to: string }) {
  const location = useLocation()
  return <Navigate to={to} replace state={location.state} />
}
