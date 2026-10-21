import { createContext } from 'react'
import type { DesktopConnection } from './desktopConnection'

export const DesktopInstanceContext = createContext<{
  select: (connection: DesktopConnection) => Promise<void>
  add: () => void
} | null>(null)
