import { Navigate, createBrowserRouter } from 'react-router-dom'

import { AppLayout } from '@/components/layout/AppLayout'
import { RequireAuth } from '@/components/layout/RequireAuth'
import { NotFoundPage } from '@/pages/NotFoundPage'
import { AgentCreatePage } from '@/pages/agents/AgentCreatePage'
import { AgentDetailPage } from '@/pages/agents/AgentDetailPage'
import { AgentsIndexPage } from '@/pages/agents/AgentsIndexPage'
import { LoginPage } from '@/pages/auth/LoginPage'
import { RegisterPage } from '@/pages/auth/RegisterPage'
import { GroupChatPage } from '@/pages/group/GroupChatPage'
import { GroupManagePage } from '@/pages/group/GroupManagePage'
import { GroupsRightPane } from '@/pages/groups/GroupsRightPane'
import { ProviderCreatePage } from '@/pages/providers/ProviderCreatePage'
import { ProviderDetailPage } from '@/pages/providers/ProviderDetailPage'
import { ProvidersIndexPage } from '@/pages/providers/ProvidersIndexPage'
import { SystemSettingsPage } from '@/pages/settings/SystemSettingsPage'
import { SkillCreatePage } from '@/pages/skills/SkillCreatePage'
import { SkillDetailPage } from '@/pages/skills/SkillDetailPage'
import { SkillsIndexPage } from '@/pages/skills/SkillsIndexPage'
import { WorkspaceCreatePage } from '@/pages/workspace/WorkspaceCreatePage'
import { WorkspaceDetailPage } from '@/pages/workspace/WorkspaceDetailPage'
import { WorkspacesIndexPage } from '@/pages/workspace/WorkspacesIndexPage'

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
          { path: '/groups/:groupId/manage', element: <GroupManagePage /> },
          { path: '/groups/:groupId', element: <GroupChatPage /> },
          { path: '/agents', element: <AgentsIndexPage /> },
          { path: '/agents/new', element: <AgentCreatePage /> },
          { path: '/agents/:agentId', element: <AgentDetailPage /> },
          { path: '/providers', element: <ProvidersIndexPage /> },
          { path: '/providers/new', element: <ProviderCreatePage /> },
          { path: '/providers/:providerId', element: <ProviderDetailPage /> },
          { path: '/skills', element: <SkillsIndexPage /> },
          { path: '/skills/new', element: <SkillCreatePage /> },
          { path: '/skills/:skillId', element: <SkillDetailPage /> },
          { path: '/workspaces', element: <WorkspacesIndexPage /> },
          { path: '/workspaces/new', element: <WorkspaceCreatePage /> },
          { path: '/workspaces/:workspaceId', element: <WorkspaceDetailPage /> },
          { path: '/settings', element: <SystemSettingsPage /> },
        ],
      },
    ],
  },
  { path: '*', element: <NotFoundPage /> },
])
