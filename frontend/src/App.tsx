import { RouterProvider } from 'react-router-dom'

import { TooltipProvider } from '@/components/ui/tooltip'
import { router } from '@/routes'

export default function App() {
  return (
    <TooltipProvider delayDuration={150} skipDelayDuration={300}>
      <RouterProvider router={router} />
    </TooltipProvider>
  )
}
