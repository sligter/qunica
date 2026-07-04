import { useEffect, useState } from 'react'

import { useSystemSettings } from '@/hooks/useSystemSettings'
import { useAuthStore } from '@/stores/authStore'
import type { Appearance } from '@/types/api'

type ResolvedAppearance = 'light' | 'dark'

function systemPrefersDark(): boolean {
  return (
    typeof window !== 'undefined' &&
    'matchMedia' in window &&
    window.matchMedia('(prefers-color-scheme: dark)').matches
  )
}

function resolveAppearance(
  appearance: Appearance,
  systemDark: boolean,
): ResolvedAppearance {
  if (appearance === 'system') return systemDark ? 'dark' : 'light'
  return appearance
}

export function useApplyAppearance(): void {
  const token = useAuthStore((s) => s.token)
  const settings = useSystemSettings()
  const appearance = token ? settings.data?.appearance ?? 'system' : 'system'
  const [systemDark, setSystemDark] = useState(systemPrefersDark)

  useEffect(() => {
    if (typeof window === 'undefined' || !('matchMedia' in window)) return
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    setSystemDark(media.matches)

    const onChange = (event: MediaQueryListEvent) => {
      setSystemDark(event.matches)
    }
    media.addEventListener('change', onChange)
    return () => media.removeEventListener('change', onChange)
  }, [])

  useEffect(() => {
    const resolved = resolveAppearance(appearance, systemDark)
    const root = document.documentElement
    root.dataset.theme = resolved
    root.style.colorScheme = resolved
  }, [appearance, systemDark])
}
