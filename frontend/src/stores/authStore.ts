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
const USER_SYNC_KEY = 'agentchat:auth-user:v1'

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

function publishUser(user: UserRead): void {
  try {
    localStorage.setItem(USER_SYNC_KEY, JSON.stringify(user))
  } catch {
    // Cross-window profile sync is best-effort; the server remains authoritative.
  }
}

function parseSyncedUser(raw: string | null): UserRead | null {
  if (!raw) return null
  try {
    const user = JSON.parse(raw) as Partial<UserRead>
    return typeof user.id === 'string'
      && typeof user.email === 'string'
      && typeof user.name === 'string'
      && (user.avatar_url === null || typeof user.avatar_url === 'string')
      && typeof user.created_at === 'string'
      ? user as UserRead
      : null
  } catch {
    return null
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
  setUser: (user) => {
    set({ user })
    if (user) publishUser(user)
  },
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
    if (event.key === USER_SYNC_KEY) {
      const user = parseSyncedUser(event.newValue)
      const current = useAuthStore.getState()
      if (!user || current.user?.id !== user.id) return
      useAuthStore.setState({ user })
      queryClient.setQueryData(['auth', 'me', current.token], user)
      void queryClient.invalidateQueries({
        predicate: ({ queryKey }) => queryKey[0] === 'groups'
          && (queryKey[2] === 'members' || queryKey[2] === 'member-candidates'),
      })
      return
    }
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
