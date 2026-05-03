import { Navigate, createBrowserRouter } from 'react-router-dom'

import { AppLayout } from '@/components/layout/AppLayout'
import { RequireAuth } from '@/components/layout/RequireAuth'
import { NotFoundPage } from '@/pages/NotFoundPage'
import { AgentsPage } from '@/pages/agents/AgentsPage'
import { LoginPage } from '@/pages/auth/LoginPage'
import { RegisterPage } from '@/pages/auth/RegisterPage'
import { GroupChatPage } from '@/pages/group/GroupChatPage'
import { GroupsRightPane } from '@/pages/groups/GroupsRightPane'
import { ProvidersPage } from '@/pages/providers/ProvidersPage'
import { SkillsPage } from '@/pages/skills/SkillsPage'

export const router = createBrowserRouter([
  { path: '/login', element: <LoginPage /> },
  { path: '/register', element: <RegisterPage /> },
  {
    element: <RequireAuth />,
    children: [
      {
        element: <AppLayout />,
        children: [
          { path: '/', element: <Navigate to="/groups" replace /> },
          { path: '/groups', element: <GroupsRightPane /> },
          { path: '/groups/:groupId', element: <GroupChatPage /> },
          { path: '/agents', element: <AgentsPage /> },
          { path: '/providers', element: <ProvidersPage /> },
          { path: '/skills', element: <SkillsPage /> },
        ],
      },
    ],
  },
  { path: '*', element: <NotFoundPage /> },
])
