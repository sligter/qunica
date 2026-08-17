/**
 * Native OS notifications.
 *
 * The desktop shell hides to the tray instead of quitting, so a reply that
 * lands while the window is away has nowhere else to surface. Tauri owns the
 * toast there — WebView2 and WKWebView do not implement the web Notification
 * API — while the browser build falls back to that API.
 *
 * Nothing here throws: a refused permission or an OS without a notification
 * daemon is a quiet no-op, never a broken conversation.
 */

import { isDesktopRuntime } from '@/lib/runtime'

export type NotificationPermissionState = 'granted' | 'denied' | 'default' | 'unsupported'

interface WebNotificationApi {
  permission: NotificationPermission
  requestPermission: () => Promise<NotificationPermission>
  new (title: string, options?: NotificationOptions): Notification
}

function webNotification(): WebNotificationApi | null {
  if (typeof window === 'undefined') return null
  const api = (window as { Notification?: unknown }).Notification
  return typeof api === 'function' ? (api as WebNotificationApi) : null
}

/** Whether this runtime can show a notification at all, permission aside. */
export function notificationsSupported(): boolean {
  return isDesktopRuntime() || webNotification() !== null
}

export function notificationPermission(): NotificationPermissionState {
  // The desktop shell asks the OS at send time; there is no state to read
  // ahead of it, and treating that as "not yet granted" would put a pointless
  // permission prompt in front of a toast that would have worked.
  if (isDesktopRuntime()) return 'granted'
  const api = webNotification()
  if (!api) return 'unsupported'
  return api.permission
}

/** Ask once, if the browser has not already answered. */
export async function requestNotificationPermission(): Promise<NotificationPermissionState> {
  if (isDesktopRuntime()) return 'granted'
  const api = webNotification()
  if (!api) return 'unsupported'
  if (api.permission !== 'default') return api.permission
  try {
    return await api.requestPermission()
  } catch {
    return 'denied'
  }
}

export async function showNotification(title: string, body: string): Promise<void> {
  if (isDesktopRuntime()) {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke('show_notification', { title, body })
    } catch {
      // An OS that refuses toasts must not break the conversation.
    }
    return
  }
  const api = webNotification()
  if (!api || api.permission !== 'granted') return
  try {
    new api(title, { body })
  } catch {
    // Same reasoning as above.
  }
}
