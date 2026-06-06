import { RouterProvider } from 'react-router-dom'

import { DesktopStartupGate } from '@/components/layout/DesktopStartupGate'
import { router } from '@/routes'

export default function App() {
  return (
    <DesktopStartupGate>
      <RouterProvider router={router} />
    </DesktopStartupGate>
  )
}
