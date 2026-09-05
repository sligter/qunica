import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { isDesktopRuntime } from '@/lib/runtime'
import { isAndroidRuntime } from '@/lib/androidSession'

export function PwaStatus() {
  const { t } = useTranslation('common')
  const [offline, setOffline] = useState(!navigator.onLine)
  const [updateReady, setUpdateReady] = useState(false)

  useEffect(() => {
    const updateNetwork = () => setOffline(!navigator.onLine)
    window.addEventListener('online', updateNetwork)
    window.addEventListener('offline', updateNetwork)
    let disposed = false
    let registration: ServiceWorkerRegistration | undefined
    let worker: ServiceWorker | null = null
    const checkUpdate = () => {
      if (!disposed && registration?.waiting) setUpdateReady(true)
    }
    const watchUpdate = () => {
      worker?.removeEventListener('statechange', checkUpdate)
      worker = registration?.installing ?? null
      worker?.addEventListener('statechange', checkUpdate)
      checkUpdate()
    }
    if (import.meta.env.PROD && !isDesktopRuntime() && !isAndroidRuntime() && window.isSecureContext && 'serviceWorker' in navigator) {
      void navigator.serviceWorker.register('/sw.js').then(value => {
        if (disposed) return
        registration = value
        registration.addEventListener('updatefound', watchUpdate)
        watchUpdate()
      }).catch(error => console.warn('Service worker registration failed', error))
    }
    return () => {
      disposed = true
      window.removeEventListener('online', updateNetwork)
      window.removeEventListener('offline', updateNetwork)
      registration?.removeEventListener('updatefound', watchUpdate)
      worker?.removeEventListener('statechange', checkUpdate)
    }
  }, [])

  if (isDesktopRuntime() || (!offline && !updateReady)) return null
  return (
    <div role="status" className="shrink-0 border-b border-border bg-muted px-4 py-2 text-center text-xs text-foreground">
      {t(offline ? 'mobile.offline' : 'mobile.updateReady')}
    </div>
  )
}
