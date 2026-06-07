import { RouterProvider } from 'react-router-dom'

import { DesktopStartupGate } from '@/components/layout/DesktopStartupGate'
import { TooltipProvider } from '@/components/ui/tooltip'
import { router } from '@/routes'

export default function App() {
  return (
    <TooltipProvider delayDuration={150} skipDelayDuration={300}>
      <DesktopStartupGate>
        <RouterProvider router={router} />
      </DesktopStartupGate>
    </TooltipProvider>
  )
}
