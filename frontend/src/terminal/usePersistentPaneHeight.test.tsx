import { act, renderHook } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { usePersistentPaneHeight } from '@/terminal/usePersistentPaneHeight'

describe('usePersistentPaneHeight', () => {
  it('clamps pointer resizing between 180px and 70 percent', () => {
    const { result } = renderHook(() => usePersistentPaneHeight({ availableHeight: 1000 }))
    act(() => result.current.resizeTo(50))
    expect(result.current.height).toBe(180)
    act(() => result.current.resizeTo(900))
    expect(result.current.height).toBe(700)
  })

  it('uses 35 percent by default and restores it', () => {
    const onPersist = vi.fn()
    const { result } = renderHook(() => usePersistentPaneHeight({
      availableHeight: 1000,
      onPersist,
    }))
    expect(result.current.height).toBe(350)
    act(() => result.current.resizeTo(500))
    act(() => result.current.reset())
    expect(result.current.height).toBe(350)
    expect(onPersist).toHaveBeenLastCalledWith(350)
  })

  it('clamps persisted height and supports keyboard steps', () => {
    const preventDefault = vi.fn()
    const { result } = renderHook(() => usePersistentPaneHeight({
      availableHeight: 1000,
      persistedHeight: 900,
    }))
    expect(result.current.height).toBe(700)
    act(() => result.current.separatorProps.onKeyDown({
      key: 'ArrowDown',
      preventDefault,
    } as never))
    expect(result.current.height).toBe(676)
    expect(preventDefault).toHaveBeenCalled()
  })

  it('removes window pointer listeners after a drag and on unmount', () => {
    const add = vi.spyOn(window, 'addEventListener')
    const remove = vi.spyOn(window, 'removeEventListener')
    const { result, unmount } = renderHook(() => usePersistentPaneHeight({ availableHeight: 1000 }))
    act(() => result.current.separatorProps.onPointerDown({
      button: 0,
      clientY: 500,
      pointerId: 1,
      preventDefault: vi.fn(),
      currentTarget: { setPointerCapture: vi.fn() },
    } as never))
    expect(add).toHaveBeenCalledWith('pointermove', expect.any(Function))
    act(() => window.dispatchEvent(new PointerEvent('pointerup')))
    expect(remove).toHaveBeenCalledWith('pointermove', expect.any(Function))
    unmount()
    add.mockRestore()
    remove.mockRestore()
  })
})
