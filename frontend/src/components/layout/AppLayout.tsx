import { Suspense } from 'react'
import { Outlet } from 'react-router-dom'

import { AppSidebar } from '@/components/layout/AppSidebar'
import { RouteFallback } from '@/components/layout/RouteFallback'
import {
  TerminalRuntimeProvider,
} from '@/terminal/TerminalRuntimeProvider'
import { TerminalDock } from '@/terminal/TerminalDock'
import type { TerminalTransport } from '@/terminal/transport'

export interface AppLayoutProps {
  terminalTransport?: TerminalTransport
}

export function AppLayout({ terminalTransport }: AppLayoutProps = {}) {
  return (
    <TerminalRuntimeProvider transport={terminalTransport}>
      <div
        className="flex h-full min-h-0 overflow-hidden bg-background"
        onContextMenu={(event) => event.preventDefault()}
      >
        <AppSidebar />
        <main className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
          <div className="min-h-0 flex-1 overflow-hidden">
            <Suspense fallback={<RouteFallback />}>
              <Outlet />
            </Suspense>
          </div>
          <TerminalDock />
        </main>
      </div>
    </TerminalRuntimeProvider>
  )
}
