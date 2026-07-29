import { act, renderHook } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { useEditSaveGuard } from './useEditSaveGuard'

describe('useEditSaveGuard', () => {
  afterEach(() => vi.useRealTimers())

  it('blocks save activation until the edit click has passed', () => {
    vi.useFakeTimers()
    const { result, rerender } = renderHook(
      ({ editing }) => useEditSaveGuard(editing),
      { initialProps: { editing: false } },
    )

    rerender({ editing: true })
    expect(result.current).toBe(false)

    act(() => vi.advanceTimersByTime(399))
    expect(result.current).toBe(false)

    act(() => vi.advanceTimersByTime(1))
    expect(result.current).toBe(true)
  })
})
