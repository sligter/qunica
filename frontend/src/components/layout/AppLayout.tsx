import { Outlet } from 'react-router-dom'

import { AppSidebar } from '@/components/layout/AppSidebar'
import {
  TerminalRuntimeProvider,
} from '@/terminal/TerminalRuntimeProvider'
import type { TerminalTransport } from '@/terminal/transport'

export interface AppLayoutProps {
  terminalTransport?: TerminalTransport
}

export function AppLayout({ terminalTransport }: AppLayoutProps = {}) {
  return (
    <TerminalRuntimeProvider transport={terminalTransport}>
      <div
        className="flex h-screen min-h-0 bg-background"
        onContextMenu={(event) => event.preventDefault()}
      >
        <AppSidebar />
        <main className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
          <div className="min-h-0 flex-1 overflow-hidden">
            <Outlet />
          </div>
          <div data-testid="terminal-dock-host" className="shrink-0" />
        </main>
      </div>
    </TerminalRuntimeProvider>
  )
}
