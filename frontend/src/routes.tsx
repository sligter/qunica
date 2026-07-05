import { Navigate, createBrowserRouter } from 'react-router-dom'

import { AppLayout } from '@/components/layout/AppLayout'
import { LegacyDetailRedirect } from '@/components/layout/LegacyDetailRedirect'
import { RequireAuth } from '@/components/layout/RequireAuth'
import { AgentsListColumn } from '@/components/layout/AgentsListColumn'
import { ProvidersListColumn } from '@/components/layout/ProvidersListColumn'
import { SettingsEntityLayout, SettingsLayout } from '@/components/layout/SettingsLayout'
import { SkillsListColumn } from '@/components/layout/SkillsListColumn'
import { WorkspacesListColumn } from '@/components/layout/WorkspacesListColumn'
import { NotFoundPage } from '@/pages/NotFoundPage'
import { AgentCreatePage } from '@/pages/agents/AgentCreatePage'
import { AgentDetailPage } from '@/pages/agents/AgentDetailPage'
import { AgentsIndexPage } from '@/pages/agents/AgentsIndexPage'
import { LoginPage } from '@/pages/auth/LoginPage'
import { RegisterPage } from '@/pages/auth/RegisterPage'
import { GroupChatPage } from '@/pages/group/GroupChatPage'
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
          {
            path: '/settings',
            element: <SettingsLayout />,
            children: [
              { index: true, element: <Navigate to="/settings/general" replace /> },
              { path: 'general', element: <SystemSettingsPage /> },
              {
                path: 'agents',
                element: (
                  <SettingsEntityLayout
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
                path: 'providers',
                element: (
                  <SettingsEntityLayout
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
                path: 'skills',
                element: (
                  <SettingsEntityLayout
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
                path: 'workspaces',
                element: (
                  <SettingsEntityLayout
                    list={<WorkspacesListColumn width={ENTITY_LIST_WIDTH} />}
                  />
                ),
                children: [
                  { index: true, element: <WorkspacesIndexPage /> },
                  { path: 'new', element: <WorkspaceCreatePage /> },
                  { path: ':workspaceId', element: <WorkspaceDetailPage /> },
                ],
              },
            ],
          },
          // Legacy top-level routes redirect into the settings surface.
          { path: '/agents', element: <Navigate to="/settings/agents" replace /> },
          { path: '/agents/new', element: <Navigate to="/settings/agents/new" replace /> },
          {
            path: '/agents/:id',
            element: <LegacyDetailRedirect base="/settings/agents" />,
          },
          { path: '/providers', element: <Navigate to="/settings/providers" replace /> },
          {
            path: '/providers/new',
            element: <Navigate to="/settings/providers/new" replace />,
          },
          {
            path: '/providers/:id',
            element: <LegacyDetailRedirect base="/settings/providers" />,
          },
          { path: '/skills', element: <Navigate to="/settings/skills" replace /> },
          { path: '/skills/new', element: <Navigate to="/settings/skills/new" replace /> },
          {
            path: '/skills/:id',
            element: <LegacyDetailRedirect base="/settings/skills" />,
          },
          {
            path: '/workspaces',
            element: <Navigate to="/settings/workspaces" replace />,
          },
          {
            path: '/workspaces/new',
            element: <Navigate to="/settings/workspaces/new" replace />,
          },
          {
            path: '/workspaces/:id',
            element: <LegacyDetailRedirect base="/settings/workspaces" />,
          },
        ],
      },
    ],
  },
  { path: '*', element: <NotFoundPage /> },
])
