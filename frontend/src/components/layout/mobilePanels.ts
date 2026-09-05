import { createContext, useCallback, useContext } from 'react'

export type MobilePanel = 'navigation' | 'workspace' | 'assistant' | 'terminal'

export const MobilePanelContext = createContext<{
  panel: MobilePanel | null
  setPanel: (panel: MobilePanel | null) => void
}>({ panel: null, setPanel: () => undefined })

export function useMobilePanel(name: MobilePanel): [boolean, (open: boolean) => void] {
  const { panel, setPanel } = useContext(MobilePanelContext)
  return [panel === name, useCallback((open: boolean) => {
    if (open || panel === name) setPanel(open ? name : null)
  }, [name, panel, setPanel])]
}
