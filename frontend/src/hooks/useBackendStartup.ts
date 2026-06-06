import { useCallback, useEffect, useState } from 'react'

import { apiUrl, isDesktopRuntime } from '@/lib/runtime'

const HEALTH_CHECK_TIMEOUT_MS = 2_500
const FAST_RETRY_MS = 400
const STEADY_RETRY_MS = 1_000
const SLOW_START_MS = 12_000
const STILL_WAITING_MS = 45_000

interface RuntimeSnapshot {
  desktop: boolean
  initialReady: boolean
}

interface BackendStartupState {
  isDesktop: boolean
  ready: boolean
  elapsedMs: number
  isChecking: boolean
  slow: boolean
  stillWaiting: boolean
  checkNow: () => void
}

export function useBackendStartup(): BackendStartupState {
  const [runtime] = useState<RuntimeSnapshot>(() => {
    const desktop = isDesktopRuntime()
    return {
      desktop,
      initialReady: !desktop,
    }
  })
  const [ready, setReady] = useState(runtime.initialReady)
  const [elapsedMs, setElapsedMs] = useState(0)
  const [isChecking, setIsChecking] = useState(false)
  const [retryKey, setRetryKey] = useState(0)

  const checkNow = useCallback(() => {
    setRetryKey((value) => value + 1)
  }, [])

  useEffect(() => {
    if (!runtime.desktop) {
      setReady(true)
      return
    }

    let cancelled = false
    let retryTimer: number | null = null
    let tickTimer: number | null = null
    const startedAt = Date.now()

    const clearTickTimer = () => {
      if (tickTimer !== null) {
        window.clearInterval(tickTimer)
        tickTimer = null
      }
    }

    const updateElapsed = () => {
      setElapsedMs(Date.now() - startedAt)
    }

    const scheduleCheck = (delayMs: number) => {
      retryTimer = window.setTimeout(runCheck, delayMs)
    }

    const runCheck = async () => {
      updateElapsed()
      setIsChecking(true)

      const controller = new AbortController()
      const timeout = window.setTimeout(() => controller.abort(), HEALTH_CHECK_TIMEOUT_MS)

      try {
        const response = await fetch(apiUrl('/api/v1/health'), {
          cache: 'no-store',
          signal: controller.signal,
        })
        if (cancelled) {
          return
        }
        if (response.ok) {
          setReady(true)
          clearTickTimer()
          return
        }
      } catch {
        // The startup screen presents a product-level waiting state; detailed
        // launch diagnostics are written by the desktop host.
      } finally {
        window.clearTimeout(timeout)
        if (!cancelled) {
          setIsChecking(false)
        }
      }

      if (!cancelled) {
        const elapsed = Date.now() - startedAt
        scheduleCheck(elapsed < SLOW_START_MS ? FAST_RETRY_MS : STEADY_RETRY_MS)
      }
    }

    setReady(false)
    setElapsedMs(0)
    tickTimer = window.setInterval(updateElapsed, 1_000)
    void runCheck()

    return () => {
      cancelled = true
      if (retryTimer !== null) {
        window.clearTimeout(retryTimer)
      }
      clearTickTimer()
    }
  }, [runtime.desktop, retryKey])

  return {
    isDesktop: runtime.desktop,
    ready,
    elapsedMs,
    isChecking,
    slow: elapsedMs >= SLOW_START_MS,
    stillWaiting: elapsedMs >= STILL_WAITING_MS,
    checkNow,
  }
}
