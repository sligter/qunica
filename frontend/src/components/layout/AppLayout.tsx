import { Outlet } from 'react-router-dom'

import { AppSidebar } from '@/components/layout/AppSidebar'

export function AppLayout() {
  return (
    <div className="flex h-screen bg-background">
      <AppSidebar />
      <main className="flex min-w-0 flex-1 flex-col overflow-hidden">
        <Outlet />
      </main>
    </div>
  )
}
