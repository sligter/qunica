import { Navigate, createBrowserRouter } from 'react-router-dom'

import { AppLayout } from '@/components/layout/AppLayout'
import { LegacyDetailRedirect } from '@/components/layout/LegacyDetailRedirect'
import { RequireAuth } from '@/components/layout/RequireAuth'
import { AgentsListColumn } from '@/components/layout/AgentsListColumn'
import { EntityLayout } from '@/components/layout/EntityLayout'
import { ProvidersListColumn } from '@/components/layout/ProvidersListColumn'
import { SettingsLayout } from '@/components/layout/SettingsLayout'
import { SkillsListColumn } from '@/components/layout/SkillsListColumn'
import { WorkspacesListColumn } from '@/components/layout/WorkspacesListColumn'
import { NotFoundPage } from '@/pages/NotFoundPage'
import { AgentCreatePage } from '@/pages/agents/AgentCreatePage'
import { AgentDetailPage } from '@/pages/agents/AgentDetailPage'
import { AgentsIndexPage } from '@/pages/agents/AgentsIndexPage'
import { LoginPage } from '@/pages/auth/LoginPage'
import { RegisterPage } from '@/pages/auth/RegisterPage'
import { GroupChatPage } from '@/pages/group/GroupChatPage'
import { DirectChatPage } from '@/pages/chat/DirectChatPage'
import { GroupManagePage } from '@/pages/group/GroupManagePage'
import { ChatHomePage } from '@/pages/home/ChatHomePage'
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

const ENTITY_LIST_WIDTH = 240

export const router = createBrowserRouter([
  { path: '/login', element: <LoginPage /> },
  { path: '/register', element: <RegisterPage /> },
  {
    element: <RequireAuth />,
    children: [
      {
        element: <AppLayout />,
        children: [
          { path: '/', element: <ChatHomePage /> },
          { path: '/groups', element: <Navigate to="/" replace /> },
          { path: '/groups/:groupId/manage', element: <GroupManagePage /> },
          { path: '/groups/:groupId', element: <GroupChatPage /> },
          { path: '/chats/:chatId', element: <DirectChatPage /> },
          // Top-level entity areas: the sidebar Library is their only entry.
          {
            path: '/agents',
            element: (
              <EntityLayout
                titleKey="agents"
                list={<AgentsListColumn width={ENTITY_LIST_WIDTH} />}
              />
            ),
            children: [
              { index: true, element: <AgentsIndexPage /> },
              { path: 'new', element: <AgentCreatePage /> },
              { path: ':agentId', element: <AgentDetailPage /> },
            ],
          },
          {
            path: '/providers',
            element: (
              <EntityLayout
                titleKey="providers"
                list={<ProvidersListColumn width={ENTITY_LIST_WIDTH} />}
              />
            ),
            children: [
              { index: true, element: <ProvidersIndexPage /> },
              { path: 'new', element: <ProviderCreatePage /> },
              { path: ':providerId', element: <ProviderDetailPage /> },
            ],
          },
          {
            path: '/skills',
            element: (
              <EntityLayout
                titleKey="skills"
                list={<SkillsListColumn width={ENTITY_LIST_WIDTH} />}
              />
            ),
            children: [
              { index: true, element: <SkillsIndexPage /> },
              { path: 'new', element: <SkillCreatePage /> },
              { path: ':skillId', element: <SkillDetailPage /> },
            ],
          },
          {
            path: '/workspaces',
            element: (
              <EntityLayout
                titleKey="workspaces"
                list={<WorkspacesListColumn width={ENTITY_LIST_WIDTH} />}
              />
            ),
            children: [
              { index: true, element: <WorkspacesIndexPage /> },
              { path: 'new', element: <WorkspaceCreatePage /> },
              { path: ':workspaceId', element: <WorkspaceDetailPage /> },
            ],
          },
          // Settings keeps only global configuration.
          {
            path: '/settings',
            element: <SettingsLayout />,
            children: [{ index: true, element: <SystemSettingsPage /> }],
          },
          { path: '/settings/general', element: <Navigate to="/settings" replace /> },
          // Legacy /settings/<area>* deep links redirect to the top-level areas.
          { path: '/settings/agents', element: <Navigate to="/agents" replace /> },
          {
            path: '/settings/agents/new',
            element: <Navigate to="/agents/new" replace />,
          },
          {
            path: '/settings/agents/:id',
            element: <LegacyDetailRedirect base="/agents" />,
          },
          { path: '/settings/providers', element: <Navigate to="/providers" replace /> },
          {
            path: '/settings/providers/new',
            element: <Navigate to="/providers/new" replace />,
          },
          {
            path: '/settings/providers/:id',
            element: <LegacyDetailRedirect base="/providers" />,
          },
          { path: '/settings/skills', element: <Navigate to="/skills" replace /> },
          {
            path: '/settings/skills/new',
            element: <Navigate to="/skills/new" replace />,
          },
          {
            path: '/settings/skills/:id',
            element: <LegacyDetailRedirect base="/skills" />,
          },
          {
            path: '/settings/workspaces',
            element: <Navigate to="/workspaces" replace />,
          },
          {
            path: '/settings/workspaces/new',
            element: <Navigate to="/workspaces/new" replace />,
          },
          {
            path: '/settings/workspaces/:id',
            element: <LegacyDetailRedirect base="/workspaces" />,
          },
        ],
      },
    ],
  },
  { path: '*', element: <NotFoundPage /> },
])
