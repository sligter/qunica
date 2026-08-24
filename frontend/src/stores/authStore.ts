/**
 * Auth state: token, current user, hydration flag.
 *
 * Persisted to `localStorage` under a versioned key so a future schema
 * change can bump the version and silently sign users out.
 */

import { create } from 'zustand'

import { queryClient } from '@/lib/queryClient'
import { useQueuedMessagesStore } from '@/stores/queuedMessagesStore'
import type { UserRead } from '@/types/api'

const STORAGE_KEY = 'agentchat:auth:v1'

interface PersistedAuth {
  token: string | null
}

function parsePersistedAuth(raw: string | null): PersistedAuth {
  if (!raw) return { token: null }
  try {
    const parsed = JSON.parse(raw) as Partial<PersistedAuth>
    return { token: typeof parsed.token === 'string' ? parsed.token : null }
  } catch {
    return { token: null }
  }
}

function loadFromStorage(): PersistedAuth {
  try {
    return parsePersistedAuth(localStorage.getItem(STORAGE_KEY))
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
    queryClient.clear()
    useQueuedMessagesStore.getState().clearAll()
    set({ token: null, user: null })
  },
}))

// Tauri auxiliary windows and ordinary browser tabs have separate Zustand
// instances but share localStorage. Mirror auth changes so logging out in the
// conversation window cannot leave a library/Assistant window authenticated
// with a stale in-memory token.
if (typeof window !== 'undefined') {
  const syncAuthFromStorage = (event: StorageEvent) => {
    if (event.key !== STORAGE_KEY) return
    const { token } = parsePersistedAuth(event.newValue)
    queryClient.clear()
    useQueuedMessagesStore.getState().clearAll()
    useAuthStore.setState({
      token,
      user: null,
      hydrated: token === null,
    })
  }
  window.addEventListener('storage', syncAuthFromStorage)
  import.meta.hot?.dispose(() => window.removeEventListener('storage', syncAuthFromStorage))
}
