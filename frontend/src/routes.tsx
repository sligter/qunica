import { Navigate, createBrowserRouter } from 'react-router-dom'

import { AppLayout } from '@/components/layout/AppLayout'
import { RequireAuth } from '@/components/layout/RequireAuth'
import { NotFoundPage } from '@/pages/NotFoundPage'
import { AgentDetailRightPane } from '@/pages/agents/AgentDetailRightPane'
import { CreateAgentRightPane } from '@/pages/agents/CreateAgentRightPane'
import { LoginPage } from '@/pages/auth/LoginPage'
import { RegisterPage } from '@/pages/auth/RegisterPage'
import { GroupChatPage } from '@/pages/group/GroupChatPage'
import { GroupsRightPane } from '@/pages/groups/GroupsRightPane'
import { CreateProviderRightPane } from '@/pages/providers/CreateProviderRightPane'
import { ProviderDetailRightPane } from '@/pages/providers/ProviderDetailRightPane'
import { CreateSkillRightPane } from '@/pages/skills/CreateSkillRightPane'
import { SkillDetailRightPane } from '@/pages/skills/SkillDetailRightPane'

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
          { path: '/agents', element: <CreateAgentRightPane /> },
          { path: '/agents/:agentId', element: <AgentDetailRightPane /> },
          { path: '/providers', element: <CreateProviderRightPane /> },
          { path: '/providers/:providerId', element: <ProviderDetailRightPane /> },
          { path: '/skills', element: <CreateSkillRightPane /> },
          { path: '/skills/:skillId', element: <SkillDetailRightPane /> },
        ],
      },
    ],
  },
  { path: '*', element: <NotFoundPage /> },
])
