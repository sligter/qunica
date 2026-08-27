import { useEffect, useState } from 'react'

import { useSystemSettings } from '@/hooks/useSystemSettings'
import { useAuthStore } from '@/stores/authStore'
import type { Appearance } from '@/types/api'

export type ResolvedAppearance = 'light' | 'dark'
export const APPEARANCE_MIRROR_KEY = 'qunica:appearance'

export function normalizeResolvedAppearance(value: unknown): ResolvedAppearance | null {
  return value === 'light' || value === 'dark' ? value : null
}

export function readAppearanceMirror(): ResolvedAppearance | null {
  try {
    return normalizeResolvedAppearance(localStorage.getItem(APPEARANCE_MIRROR_KEY))
  } catch {
    return null
  }
}

export function writeAppearanceMirror(appearance: ResolvedAppearance): void {
  try {
    if (localStorage.getItem(APPEARANCE_MIRROR_KEY) !== appearance) {
      localStorage.setItem(APPEARANCE_MIRROR_KEY, appearance)
    }
  } catch {
    // Persistence failure must not block rendering.
  }
}

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

function applyResolvedAppearance(appearance: ResolvedAppearance): void {
  const root = document.documentElement
  root.dataset.theme = appearance
  root.style.colorScheme = appearance
}

export function useApplyAppearance(): void {
  const token = useAuthStore((s) => s.token)
  const currentUserId = useAuthStore((s) => s.user?.id)
  const settings = useSystemSettings()
  const appearance = token === null
    ? 'system'
    : currentUserId !== undefined && settings.data?.owner_id === currentUserId
      ? settings.data.appearance
      : undefined
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
    // Keep the theme painted by index.html while authenticated settings are
    // still loading. Falling back to `system` here would create a second flash
    // for accounts that explicitly selected the other theme.
    if (!appearance) return
    const resolved = resolveAppearance(appearance, systemDark)
    applyResolvedAppearance(resolved)
    writeAppearanceMirror(resolved)
  }, [appearance, systemDark])

  useEffect(() => {
    const syncAppearance = (event: StorageEvent) => {
      if (event.key !== APPEARANCE_MIRROR_KEY) return
      const resolved = normalizeResolvedAppearance(event.newValue)
      if (resolved) applyResolvedAppearance(resolved)
    }
    window.addEventListener('storage', syncAppearance)
    return () => window.removeEventListener('storage', syncAppearance)
  }, [])
}
