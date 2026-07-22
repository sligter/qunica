import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
} from 'react'

const DEFAULT_HEIGHT_RATIO = 0.35
const MAX_HEIGHT_RATIO = 0.7
const MIN_HEIGHT = 180
const KEYBOARD_STEP = 24

export interface UsePersistentPaneHeightOptions {
  availableHeight: number
  persistedHeight?: number
  onPersist?: (height: number) => void
}

export interface PersistentPaneHeight {
  height: number
  resizeTo(height: number): void
  reset(): void
  separatorProps: {
    onKeyDown(event: ReactKeyboardEvent<HTMLElement>): void
    onPointerDown(event: ReactPointerEvent<HTMLElement>): void
  }
}

function usableHeight(value: number): number {
  return Number.isFinite(value) && value > 0 ? value : 0
}

function bounds(availableHeight: number): { min: number; max: number } {
  const available = usableHeight(availableHeight)
  if (available === 0) return { min: MIN_HEIGHT, max: MIN_HEIGHT }
  const max = Math.round(available * MAX_HEIGHT_RATIO)
  return { min: Math.min(MIN_HEIGHT, max), max }
}

function clampHeight(height: number, availableHeight: number): number {
  const { min, max } = bounds(availableHeight)
  return Math.min(max, Math.max(min, Math.round(height)))
}

function defaultHeight(availableHeight: number): number {
  return clampHeight(usableHeight(availableHeight) * DEFAULT_HEIGHT_RATIO, availableHeight)
}

export function usePersistentPaneHeight({
  availableHeight,
  persistedHeight = 0,
  onPersist,
}: UsePersistentPaneHeightOptions): PersistentPaneHeight {
  const initialHeight = persistedHeight > 0
    ? clampHeight(persistedHeight, availableHeight)
    : defaultHeight(availableHeight)
  const [height, setHeight] = useState(initialHeight)
  const heightRef = useRef(initialHeight)
  const dragCleanupRef = useRef<(() => void) | null>(null)
  const onPersistRef = useRef(onPersist)
  onPersistRef.current = onPersist

  const applyHeight = useCallback((nextHeight: number, persist = true) => {
    const next = clampHeight(nextHeight, availableHeight)
    heightRef.current = next
    setHeight(next)
    if (persist) onPersistRef.current?.(next)
  }, [availableHeight])

  useEffect(() => {
    const preferred = persistedHeight > 0 ? persistedHeight : heightRef.current
    applyHeight(preferred, false)
  }, [applyHeight, persistedHeight])

  useEffect(() => () => dragCleanupRef.current?.(), [])

  const resizeTo = useCallback((nextHeight: number) => {
    applyHeight(nextHeight)
  }, [applyHeight])

  const reset = useCallback(() => {
    applyHeight(defaultHeight(availableHeight))
  }, [applyHeight, availableHeight])

  const onKeyDown = useCallback((event: ReactKeyboardEvent<HTMLElement>) => {
    if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return
    event.preventDefault()
    const direction = event.key === 'ArrowUp' ? 1 : -1
    applyHeight(heightRef.current + direction * KEYBOARD_STEP)
  }, [applyHeight])

  const onPointerDown = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    if (event.button !== 0) return
    event.preventDefault()
    dragCleanupRef.current?.()
    const startY = event.clientY
    const startHeight = heightRef.current
    const target = event.currentTarget
    target.setPointerCapture?.(event.pointerId)

    const onPointerMove = (moveEvent: PointerEvent) => {
      applyHeight(startHeight + startY - moveEvent.clientY)
    }
    const cleanup = () => {
      window.removeEventListener('pointermove', onPointerMove)
      window.removeEventListener('pointerup', cleanup)
      window.removeEventListener('pointercancel', cleanup)
      dragCleanupRef.current = null
    }
    dragCleanupRef.current = cleanup
    window.addEventListener('pointermove', onPointerMove)
    window.addEventListener('pointerup', cleanup)
    window.addEventListener('pointercancel', cleanup)
  }, [applyHeight])

  const separatorProps = useMemo(() => ({ onKeyDown, onPointerDown }), [onKeyDown, onPointerDown])

  return { height, resizeTo, reset, separatorProps }
}
