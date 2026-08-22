import { useCallback, useMemo, useRef, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import {
  UnsavedChangesContext,
  type UnsavedChangesContextValue,
} from '@/hooks/useUnsavedChangesGuard'

/** Owns the single confirmation dialog shared by every buffered overlay form. */
export function UnsavedChangesProvider({ children }: { children: ReactNode }) {
  const { t } = useTranslation('common')
  const sources = useRef(new Set<symbol>())
  const [dirty, setDirty] = useState(false)
  const [pendingAction, setPendingAction] = useState<(() => void) | null>(null)

  const setSourceDirty = useCallback((source: symbol, next: boolean) => {
    if (next) sources.current.add(source)
    else sources.current.delete(source)
    setDirty(sources.current.size > 0)
  }, [])

  const requestAction = useCallback((action: () => void) => {
    if (!dirty) {
      action()
      return
    }
    setPendingAction(() => action)
  }, [dirty])

  const value = useMemo<UnsavedChangesContextValue>(
    () => ({ setSourceDirty, requestAction }),
    [requestAction, setSourceDirty],
  )

  return (
    <UnsavedChangesContext.Provider value={value}>
      {children}
      <ConfirmDialog
        open={pendingAction !== null}
        onOpenChange={(open) => {
          if (!open) setPendingAction(null)
        }}
        title={t('unsavedChanges.title')}
        description={t('unsavedChanges.description')}
        confirmLabel={t('unsavedChanges.discard')}
        destructive
        onConfirm={() => {
          const action = pendingAction
          setPendingAction(null)
          action?.()
        }}
      />
    </UnsavedChangesContext.Provider>
  )
}
