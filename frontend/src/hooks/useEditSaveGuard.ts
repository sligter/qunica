import { useEffect, useState } from 'react'

const SAVE_GUARD_MS = 400

export function useEditSaveGuard(editing: boolean): boolean {
  const [ready, setReady] = useState(false)

  useEffect(() => {
    setReady(false)
    if (!editing) return
    const timer = window.setTimeout(() => setReady(true), SAVE_GUARD_MS)
    return () => window.clearTimeout(timer)
  }, [editing])

  return editing && ready
}
