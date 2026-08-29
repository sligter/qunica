import { createBrowserRouter } from 'react-router-dom'

import { AppLayout } from '@/components/layout/AppLayout'
import { RequireAuth } from '@/components/layout/RequireAuth'
import { FirstRunGate } from '@/components/onboarding/FirstRunGate'
import { appChildren } from '@/routes/appRoutes'
import { NotFoundPage } from '@/pages/NotFoundPage'
import { AssistantDockWindow } from '@/pages/assistant/AssistantDockWindow'
import { LoginPage } from '@/pages/auth/LoginPage'
import { RegisterPage } from '@/pages/auth/RegisterPage'

export const router = createBrowserRouter([
  { path: '/login', element: <LoginPage /> },
  { path: '/register', element: <RegisterPage /> },
  {
    element: <RequireAuth />,
    children: [
      // The desktop Assistant utility window: a bare chat surface, not the
      // conversation shell. It still needs the same auth guard as everything
      // else so a stale token lands on /login instead of a half-built dock.
      { path: '/assistant-dock', element: <AssistantDockWindow /> },
      {
        element: <FirstRunGate />,
        children: [
          {
            element: <AppLayout />,
            children: appChildren,
          },
        ],
      },
    ],
  },
  { path: '*', element: <NotFoundPage /> },
])
