import { Navigate, createBrowserRouter } from 'react-router-dom'

import { AppLayout } from '@/components/layout/AppLayout'
import { RequireAuth } from '@/components/layout/RequireAuth'
import { NotFoundPage } from '@/pages/NotFoundPage'
import { AgentsPage } from '@/pages/agents/AgentsPage'
import { LoginPage } from '@/pages/auth/LoginPage'
import { RegisterPage } from '@/pages/auth/RegisterPage'
import { GroupChatPage } from '@/pages/group/GroupChatPage'
import { GroupFilesPage } from '@/pages/group/GroupFilesPage'
import { GroupMembersPage } from '@/pages/group/GroupMembersPage'
import { GroupNotesPage } from '@/pages/group/GroupNotesPage'
import { GroupsRightPane } from '@/pages/groups/GroupsRightPane'
import { ProvidersPage } from '@/pages/providers/ProvidersPage'
import { SettingsRedirectPage } from '@/pages/settings/SettingsRedirectPage'
import { SystemSettingsPage } from '@/pages/settings/SystemSettingsPage'
import { SkillsPage } from '@/pages/skills/SkillsPage'
import { WorkspacesPage } from '@/pages/workspace/WorkspacesPage'

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
          { path: '/groups/:groupId/members', element: <GroupMembersPage /> },
          { path: '/groups/:groupId/files', element: <GroupFilesPage /> },
          { path: '/groups/:groupId/notes', element: <GroupNotesPage /> },
          { path: '/groups/:groupId', element: <GroupChatPage /> },
          { path: '/agents', element: <AgentsPage /> },
          { path: '/providers', element: <ProvidersPage /> },
          { path: '/skills', element: <SkillsPage /> },
          { path: '/workspaces', element: <WorkspacesPage /> },
          { path: '/settings', element: <SettingsRedirectPage /> },
          { path: '/settings/system', element: <SystemSettingsPage /> },
        ],
      },
    ],
  },
  { path: '*', element: <NotFoundPage /> },
])
