import { RouterProvider } from 'react-router-dom'

import { TooltipProvider } from '@/components/ui/tooltip'
import { useApplyAppearance } from '@/hooks/useApplyAppearance'
import { useApplyLanguage } from '@/hooks/useApplyLanguage'
import { useSuppressNativeContextMenu } from '@/hooks/useSuppressNativeContextMenu'
import { router } from '@/routes'
import { PwaStatus } from '@/components/layout/PwaStatus'

export default function App() {
  useApplyAppearance()
  useApplyLanguage()
  useSuppressNativeContextMenu()

  return (
    <TooltipProvider delayDuration={150} skipDelayDuration={300}>
      <PwaStatus />
      <div className="min-h-0 flex-1 overflow-hidden">
        <RouterProvider router={router} />
      </div>
    </TooltipProvider>
  )
}
