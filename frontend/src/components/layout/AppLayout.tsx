import { Outlet } from 'react-router-dom'

import { MiddleColumn } from '@/components/layout/MiddleColumn'
import { SidebarRail } from '@/components/layout/SidebarRail'

export function AppLayout() {
  return (
    <div className="flex h-screen bg-background">
      <SidebarRail />
      <MiddleColumn />
      <main className="flex min-w-0 flex-1 flex-col overflow-hidden">
        <Outlet />
      </main>
    </div>
  )
}
