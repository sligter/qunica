/**
 * Auth state: token, current user, hydration flag.
 *
 * Persisted to `localStorage` under a versioned key so a future schema
 * change can bump the version and silently sign users out.
 */

import { create } from 'zustand'

import type { UserRead } from '@/types/api'

const STORAGE_KEY = 'agentchat:auth:v1'

interface PersistedAuth {
  token: string | null
}

function loadFromStorage(): PersistedAuth {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return { token: null }
    const parsed = JSON.parse(raw) as Partial<PersistedAuth>
    return { token: typeof parsed.token === 'string' ? parsed.token : null }
  } catch {
    return { token: null }
  }
}

function saveToStorage(value: PersistedAuth): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(value))
  } catch {
    // localStorage may be disabled; degrade silently.
  }
}

function clearStorage(): void {
  try {
    localStorage.removeItem(STORAGE_KEY)
  } catch {
    // ignore
  }
}

interface AuthState {
  token: string | null
  user: UserRead | null
  hydrated: boolean
  setToken: (token: string) => void
  setUser: (user: UserRead | null) => void
  setHydrated: (value: boolean) => void
  logout: () => void
}

export const useAuthStore = create<AuthState>((set) => ({
  token: loadFromStorage().token,
  user: null,
  hydrated: false,
  setToken: (token) => {
    saveToStorage({ token })
    set({ token })
  },
  setUser: (user) => set({ user }),
  setHydrated: (value) => set({ hydrated: value }),
  logout: () => {
    clearStorage()
    set({ token: null, user: null })
  },
}))
