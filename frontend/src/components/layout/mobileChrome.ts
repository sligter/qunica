import { createContext, type ReactNode } from 'react'

export const AndroidConnectionContext = createContext<{
  address: string
  paired: boolean
  edit: () => void
} | null>(null)

/** Note: the main chat owns its mobile header; the shell supplies its global
 * actions. Registering keeps loading/error pages navigable without two bars. */
export const MobileHeaderContext = createContext<{
  navigation: ReactNode
  assistant: ReactNode
  register: () => () => void
} | null>(null)
