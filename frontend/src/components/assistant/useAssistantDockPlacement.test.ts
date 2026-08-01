import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import {
  ASSISTANT_PLACEMENT_KEY,
  MIN_DOCK_HEIGHT,
  MIN_DOCK_WIDTH,
  MIN_VISIBLE_EDGE,
  useAssistantDockPlacement,
} from '@/components/assistant/useAssistantDockPlacement'

function setViewport(width: number, height: number) {
  Object.defineProperty(window, 'innerWidth', { value: width, writable: true, configurable: true })
  Object.defineProperty(window, 'innerHeight', { value: height, writable: true, configurable: true })
}

describe('useAssistantDockPlacement', () => {
  beforeEach(() => {
    localStorage.clear()
    setViewport(1440, 900)
  })

  afterEach(() => {
    localStorage.clear()
  })

  it('starts collapsed so a first-time user is not covered by a panel', () => {
    const { result } = renderHook(() => useAssistantDockPlacement())
    expect(result.current.placement.collapsed).toBe(true)
  })

  it('persists placement across a remount', () => {
    const first = renderHook(() => useAssistantDockPlacement())
    act(() => {
      first.result.current.setPlacement({ collapsed: false, width: 420, height: 620 })
    })
    first.unmount()

    const second = renderHook(() => useAssistantDockPlacement())
    expect(second.result.current.placement.collapsed).toBe(false)
    expect(second.result.current.placement.width).toBe(420)
    expect(second.result.current.placement.height).toBe(620)
  })

  it('clamps a placement saved on a larger screen back into view', () => {
    localStorage.setItem(
      ASSISTANT_PLACEMENT_KEY,
      JSON.stringify({ x: 2400, y: 1800, width: 900, height: 1200, collapsed: false }),
    )
    setViewport(800, 600)

    const { result } = renderHook(() => useAssistantDockPlacement())

    // A dock parked off a monitor that is no longer attached must not be
    // stranded where it can never be dragged back.
    expect(result.current.placement.x).toBeLessThanOrEqual(800 - MIN_VISIBLE_EDGE)
    expect(result.current.placement.y).toBeLessThanOrEqual(600 - MIN_VISIBLE_EDGE)
    expect(result.current.placement.width).toBeLessThanOrEqual(800)
    expect(result.current.placement.height).toBeLessThanOrEqual(600)
  })

  it('re-clamps when the window shrinks', () => {
    const { result } = renderHook(() => useAssistantDockPlacement())
    act(() => {
      result.current.setPlacement({ collapsed: false, x: 1200, y: 700 })
    })

    act(() => {
      setViewport(500, 400)
      window.dispatchEvent(new Event('resize'))
    })

    expect(result.current.placement.x).toBeLessThanOrEqual(500 - MIN_VISIBLE_EDGE)
    expect(result.current.placement.y).toBeLessThanOrEqual(400 - MIN_VISIBLE_EDGE)
  })

  it('never shrinks below the minimum usable size', () => {
    const { result } = renderHook(() => useAssistantDockPlacement())
    act(() => {
      result.current.setPlacement({ width: 10, height: 10 })
    })
    expect(result.current.placement.width).toBeGreaterThanOrEqual(MIN_DOCK_WIDTH)
    expect(result.current.placement.height).toBeGreaterThanOrEqual(MIN_DOCK_HEIGHT)
  })

  it('survives corrupt stored JSON', () => {
    localStorage.setItem(ASSISTANT_PLACEMENT_KEY, '{not json')
    const { result } = renderHook(() => useAssistantDockPlacement())
    expect(result.current.placement.collapsed).toBe(true)
    expect(result.current.placement.width).toBeGreaterThanOrEqual(MIN_DOCK_WIDTH)
  })

  it('snaps to the nearest corner', () => {
    const { result } = renderHook(() => useAssistantDockPlacement())
    act(() => {
      result.current.setPlacement({ collapsed: false, x: 20, y: 20 })
      result.current.snapToNearestCorner()
    })
    expect(result.current.placement.corner).toBe('top-left')

    act(() => {
      result.current.setPlacement({ x: 1400, y: 860 })
      result.current.snapToNearestCorner()
    })
    expect(result.current.placement.corner).toBe('bottom-right')
  })
})
