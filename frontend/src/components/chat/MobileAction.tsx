import { createContext, useContext, type ReactNode } from 'react'
import { createPortal } from 'react-dom'

// eslint-disable-next-line react-refresh/only-export-components
export const MobileActionContext = createContext<HTMLElement | null>(null)

/** Move the existing control; don't mount a second form with independent state. */
export function MobileAction({ active, children }: { active: boolean; children: ReactNode }) {
  const target = useContext(MobileActionContext)
  return active && target ? createPortal(children, target) : children
}
