import { RouterProvider } from 'react-router-dom'

import { TooltipProvider } from '@/components/ui/tooltip'
import { useApplyAppearance } from '@/hooks/useApplyAppearance'
import { useApplyLanguage } from '@/hooks/useApplyLanguage'
import { useSuppressNativeContextMenu } from '@/hooks/useSuppressNativeContextMenu'
import { router } from '@/routes'

export default function App() {
  useApplyAppearance()
  useApplyLanguage()
  useSuppressNativeContextMenu()

  return (
    <TooltipProvider delayDuration={150} skipDelayDuration={300}>
      <RouterProvider router={router} />
    </TooltipProvider>
  )
}
