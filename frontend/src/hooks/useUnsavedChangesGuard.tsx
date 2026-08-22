import {
  createContext,
  useContext,
  useEffect,
  useRef,
} from 'react'

export interface UnsavedChangesContextValue {
  setSourceDirty: (source: symbol, dirty: boolean) => void
  requestAction: (action: () => void) => void
}

export const UnsavedChangesContext = createContext<UnsavedChangesContextValue>({
  setSourceDirty: () => undefined,
  requestAction: (action) => action(),
})

/** Registers only buffered edits; instant-save controls never call this hook. */
export function useUnsavedChangesGuard(dirty: boolean): void {
  const { setSourceDirty } = useContext(UnsavedChangesContext)
  const source = useRef(Symbol('unsaved-changes'))

  useEffect(() => {
    const id = source.current
    setSourceDirty(id, dirty)
    return () => setSourceDirty(id, false)
  }, [dirty, setSourceDirty])

  useEffect(() => {
    if (!dirty) return
    const preventUnload = (event: BeforeUnloadEvent) => {
      event.preventDefault()
      event.returnValue = ''
    }
    window.addEventListener('beforeunload', preventUnload)
    return () => window.removeEventListener('beforeunload', preventUnload)
  }, [dirty])
}

/** Runs immediately when clean, or after the user confirms discarding edits. */
export function useUnsavedChangesAction(): (action: () => void) => void {
  return useContext(UnsavedChangesContext).requestAction
}
