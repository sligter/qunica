import { useCallback, useEffect, useState, type PointerEvent as ReactPointerEvent } from 'react'

interface UsePersistentPaneWidthOptions {
  storageKey: string
  defaultWidth: number
  minWidth: number
  maxWidth: number
}

interface PersistentPaneWidth {
  width: number
  minWidth: number
  maxWidth: number
  startResize: (event: ReactPointerEvent<HTMLElement>, direction?: 1 | -1) => void
  resizeBy: (delta: number) => void
}

function clampWidth(value: number, minWidth: number, maxWidth: number): number {
  return Math.min(maxWidth, Math.max(minWidth, Math.round(value)))
}

function readStoredWidth({
  storageKey,
  defaultWidth,
  minWidth,
  maxWidth,
}: UsePersistentPaneWidthOptions): number {
  if (typeof window === 'undefined') return defaultWidth
  try {
    const raw = window.localStorage.getItem(storageKey)
    if (raw === null) return defaultWidth
    const stored = Number(raw)
    if (!Number.isFinite(stored)) return defaultWidth
    return clampWidth(stored, minWidth, maxWidth)
  } catch {
    return defaultWidth
  }
}

export function usePersistentPaneWidth(
  options: UsePersistentPaneWidthOptions,
): PersistentPaneWidth {
  const { storageKey, defaultWidth, minWidth, maxWidth } = options
  const [width, setWidth] = useState(() =>
    readStoredWidth({ storageKey, defaultWidth, minWidth, maxWidth }),
  )

  const setClampedWidth = useCallback(
    (nextWidth: number) => setWidth(clampWidth(nextWidth, minWidth, maxWidth)),
    [maxWidth, minWidth],
  )

  useEffect(() => {
    try {
      window.localStorage.setItem(storageKey, String(width))
    } catch {
      // Layout preference persistence should not block the chat UI.
    }
  }, [storageKey, width])

  const resizeBy = useCallback(
    (delta: number) => {
      setWidth((current) => clampWidth(current + delta, minWidth, maxWidth))
    },
    [maxWidth, minWidth],
  )

  const startResize = useCallback(
    (event: ReactPointerEvent<HTMLElement>, direction: 1 | -1 = 1) => {
      event.preventDefault()
      const startX = event.clientX
      const startWidth = width
      const previousCursor = document.body.style.cursor
      const previousUserSelect = document.body.style.userSelect
      document.body.style.cursor = 'col-resize'
      document.body.style.userSelect = 'none'

      const stopResize = () => {
        document.body.style.cursor = previousCursor
        document.body.style.userSelect = previousUserSelect
        window.removeEventListener('pointermove', onPointerMove)
        window.removeEventListener('pointerup', stopResize)
        window.removeEventListener('pointercancel', stopResize)
      }

      const onPointerMove = (moveEvent: PointerEvent) => {
        const delta = (moveEvent.clientX - startX) * direction
        setClampedWidth(startWidth + delta)
      }

      window.addEventListener('pointermove', onPointerMove)
      window.addEventListener('pointerup', stopResize)
      window.addEventListener('pointercancel', stopResize)
    },
    [setClampedWidth, width],
  )

  return {
    width,
    minWidth,
    maxWidth,
    startResize,
    resizeBy,
  }
}
