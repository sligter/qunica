import { Outlet } from 'react-router-dom'

import { AppSidebar } from '@/components/layout/AppSidebar'

export function AppLayout() {
  return (
    <div
      className="flex h-screen min-h-0 bg-background"
      onContextMenu={(event) => event.preventDefault()}
    >
      <AppSidebar />
      <main className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
        <Outlet />
      </main>
    </div>
  )
}
