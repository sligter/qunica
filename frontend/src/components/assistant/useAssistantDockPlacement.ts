/**
 * Position, size and collapsed state for the Assistant dock.
 *
 * Follows the read-clamp-write shape of `usePersistentPaneWidth`, with one
 * addition that matters: the placement is clamped on load *and* on resize. A
 * dock parked on a second monitor that is later unplugged would otherwise be
 * stranded at coordinates the user can never drag it back from.
 */

import { useCallback, useEffect, useState } from 'react'

export const ASSISTANT_PLACEMENT_KEY = 'ag-swarmer:assistant:placement'

export const MIN_DOCK_WIDTH = 300
export const MIN_DOCK_HEIGHT = 320
export const DEFAULT_DOCK_WIDTH = 380
export const DEFAULT_DOCK_HEIGHT = 560

/** How much of the dock must stay on screen for it to remain grabbable. */
export const MIN_VISIBLE_EDGE = 48

/** Distance from a corner, in px, within which a drag snaps to it. */
const SNAP_THRESHOLD = 72

/** Gap kept between a snapped dock and the window edge. */
const EDGE_MARGIN = 16

export type DockCorner = 'top-left' | 'top-right' | 'bottom-left' | 'bottom-right'

export interface DockPlacement {
  x: number
  y: number
  width: number
  height: number
  corner: DockCorner
  collapsed: boolean
}

function viewport(): { width: number; height: number } {
  if (typeof window === 'undefined') return { width: 1280, height: 800 }
  return { width: window.innerWidth, height: window.innerHeight }
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, Math.round(value)))
}

function defaultPlacement(): DockPlacement {
  const { width, height } = viewport()
  return {
    // Bottom-right by default: it is the corner least likely to cover the
    // message list or the sidebar.
    x: Math.max(EDGE_MARGIN, width - DEFAULT_DOCK_WIDTH - EDGE_MARGIN),
    y: Math.max(EDGE_MARGIN, height - DEFAULT_DOCK_HEIGHT - EDGE_MARGIN),
    width: DEFAULT_DOCK_WIDTH,
    height: DEFAULT_DOCK_HEIGHT,
    corner: 'bottom-right',
    // Collapsed on first run. Expanding by default would put a panel over the
    // app before the user has asked for one.
    collapsed: true,
  }
}

function clampPlacement(placement: DockPlacement): DockPlacement {
  const { width: vw, height: vh } = viewport()
  const width = clamp(placement.width, MIN_DOCK_WIDTH, Math.max(MIN_DOCK_WIDTH, vw - EDGE_MARGIN))
  const height = clamp(
    placement.height,
    MIN_DOCK_HEIGHT,
    Math.max(MIN_DOCK_HEIGHT, vh - EDGE_MARGIN),
  )
  return {
    ...placement,
    width,
    height,
    // Allow the dock to hang off the right/bottom edge, but never so far that
    // less than MIN_VISIBLE_EDGE of it is reachable.
    x: clamp(placement.x, EDGE_MARGIN - width + MIN_VISIBLE_EDGE, Math.max(0, vw - MIN_VISIBLE_EDGE)),
    y: clamp(placement.y, 0, Math.max(0, vh - MIN_VISIBLE_EDGE)),
  }
}

function readStored(): DockPlacement {
  const fallback = defaultPlacement()
  if (typeof window === 'undefined') return fallback
  try {
    const raw = window.localStorage.getItem(ASSISTANT_PLACEMENT_KEY)
    if (!raw) return fallback
    const parsed = JSON.parse(raw) as Partial<DockPlacement>
    return clampPlacement({
      x: typeof parsed.x === 'number' ? parsed.x : fallback.x,
      y: typeof parsed.y === 'number' ? parsed.y : fallback.y,
      width: typeof parsed.width === 'number' ? parsed.width : fallback.width,
      height: typeof parsed.height === 'number' ? parsed.height : fallback.height,
      corner: parsed.corner ?? fallback.corner,
      collapsed: typeof parsed.collapsed === 'boolean' ? parsed.collapsed : fallback.collapsed,
    })
  } catch {
    // Corrupt storage must not stop the dock rendering.
    return fallback
  }
}

export interface AssistantDockPlacement {
  placement: DockPlacement
  setPlacement: (patch: Partial<DockPlacement>) => void
  snapToNearestCorner: () => void
  toggleCollapsed: () => void
}

export function useAssistantDockPlacement(): AssistantDockPlacement {
  const [placement, setState] = useState<DockPlacement>(readStored)

  useEffect(() => {
    try {
      window.localStorage.setItem(ASSISTANT_PLACEMENT_KEY, JSON.stringify(placement))
    } catch {
      // A layout preference is not worth failing the UI over.
    }
  }, [placement])

  useEffect(() => {
    const onResize = () => setState((current) => clampPlacement(current))
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [])

  const setPlacement = useCallback((patch: Partial<DockPlacement>) => {
    setState((current) => clampPlacement({ ...current, ...patch }))
  }, [])

  const snapToNearestCorner = useCallback(() => {
    setState((current) => {
      const { width: vw, height: vh } = viewport()
      const nearLeft = current.x <= vw / 2
      const nearTop = current.y <= vh / 2
      const distanceX = nearLeft ? current.x : vw - (current.x + current.width)
      const distanceY = nearTop ? current.y : vh - (current.y + current.height)
      // Only snap when the drag ended close to an edge. Otherwise the dock
      // would jump away from wherever the user deliberately put it.
      if (distanceX > SNAP_THRESHOLD && distanceY > SNAP_THRESHOLD) return current

      const corner: DockCorner = `${nearTop ? 'top' : 'bottom'}-${
        nearLeft ? 'left' : 'right'
      }` as DockCorner
      return clampPlacement({
        ...current,
        corner,
        x: nearLeft ? EDGE_MARGIN : vw - current.width - EDGE_MARGIN,
        y: nearTop ? EDGE_MARGIN : vh - current.height - EDGE_MARGIN,
      })
    })
  }, [])

  const toggleCollapsed = useCallback(() => {
    setState((current) => clampPlacement({ ...current, collapsed: !current.collapsed }))
  }, [])

  return { placement, setPlacement, snapToNearestCorner, toggleCollapsed }
}
