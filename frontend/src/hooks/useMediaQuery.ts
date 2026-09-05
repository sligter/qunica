import { useCallback, useSyncExternalStore } from 'react'

export function useMediaQuery(query: string, fallback = false): boolean {
  const subscribe = useCallback((notify: () => void) => {
    const media = window.matchMedia?.(query)
    media?.addEventListener('change', notify)
    return () => media?.removeEventListener('change', notify)
  }, [query])
  return useSyncExternalStore(subscribe,
    () => window.matchMedia?.(query).matches ?? fallback,
    () => fallback)
}

export function useCompactLayout(): boolean {
  return !useMediaQuery('(min-width: 1024px)', true)
}
