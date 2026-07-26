import { lazy } from 'react'
import { Navigate, createBrowserRouter } from 'react-router-dom'

import { AppLayout } from '@/components/layout/AppLayout'
import { LegacyDetailRedirect } from '@/components/layout/LegacyDetailRedirect'
import { RequireAuth } from '@/components/layout/RequireAuth'
import { AgentsListColumn } from '@/components/layout/AgentsListColumn'
import { EntityLayout } from '@/components/layout/EntityLayout'
import { McpServersListColumn } from '@/components/layout/McpServersListColumn'
import { ProvidersListColumn } from '@/components/layout/ProvidersListColumn'
import { SettingsLayout } from '@/components/layout/SettingsLayout'
import { SkillsListColumn } from '@/components/layout/SkillsListColumn'
import { WorkspacesListColumn } from '@/components/layout/WorkspacesListColumn'
import { NotFoundPage } from '@/pages/NotFoundPage'
import { LoginPage } from '@/pages/auth/LoginPage'
import { RegisterPage } from '@/pages/auth/RegisterPage'
import { GroupChatPage } from '@/pages/group/GroupChatPage'
import { DirectChatPage } from '@/pages/chat/DirectChatPage'
import { ChatHomePage } from '@/pages/home/ChatHomePage'

// Auth, layout shells, and the three chat surfaces stay eager: they are the
// landing surfaces and splitting them would only add a blank frame on boot.
// Everything below is reachable solely by navigation, so its code — including
// the react-hook-form/zod form machinery — loads when the route is entered.
const AgentCreatePage = lazy(() =>
  import('@/pages/agents/AgentCreatePage').then((m) => ({ default: m.AgentCreatePage })),
)
const AgentDetailPage = lazy(() =>
  import('@/pages/agents/AgentDetailPage').then((m) => ({ default: m.AgentDetailPage })),
)
const AgentsIndexPage = lazy(() =>
  import('@/pages/agents/AgentsIndexPage').then((m) => ({ default: m.AgentsIndexPage })),
)
const ProviderCreatePage = lazy(() =>
  import('@/pages/providers/ProviderCreatePage').then((m) => ({ default: m.ProviderCreatePage })),
)
const ProviderDetailPage = lazy(() =>
  import('@/pages/providers/ProviderDetailPage').then((m) => ({ default: m.ProviderDetailPage })),
)
const ProvidersIndexPage = lazy(() =>
  import('@/pages/providers/ProvidersIndexPage').then((m) => ({ default: m.ProvidersIndexPage })),
)
const McpServerCreatePage = lazy(() =>
  import('@/pages/mcp/McpServerCreatePage').then((m) => ({ default: m.McpServerCreatePage })),
)
const McpServerDetailPage = lazy(() =>
  import('@/pages/mcp/McpServerDetailPage').then((m) => ({ default: m.McpServerDetailPage })),
)
const McpServersIndexPage = lazy(() =>
  import('@/pages/mcp/McpServersIndexPage').then((m) => ({ default: m.McpServersIndexPage })),
)
const SkillCreatePage = lazy(() =>
  import('@/pages/skills/SkillCreatePage').then((m) => ({ default: m.SkillCreatePage })),
)
const SkillDetailPage = lazy(() =>
  import('@/pages/skills/SkillDetailPage').then((m) => ({ default: m.SkillDetailPage })),
)
const SkillsIndexPage = lazy(() =>
  import('@/pages/skills/SkillsIndexPage').then((m) => ({ default: m.SkillsIndexPage })),
)
const WorkspaceCreatePage = lazy(() =>
  import('@/pages/workspace/WorkspaceCreatePage').then((m) => ({ default: m.WorkspaceCreatePage })),
)
const WorkspaceDetailPage = lazy(() =>
  import('@/pages/workspace/WorkspaceDetailPage').then((m) => ({ default: m.WorkspaceDetailPage })),
)
const WorkspacesIndexPage = lazy(() =>
  import('@/pages/workspace/WorkspacesIndexPage').then((m) => ({ default: m.WorkspacesIndexPage })),
)
const SystemSettingsPage = lazy(() =>
  import('@/pages/settings/SystemSettingsPage').then((m) => ({ default: m.SystemSettingsPage })),
)
const GroupManagePage = lazy(() =>
  import('@/pages/group/GroupManagePage').then((m) => ({ default: m.GroupManagePage })),
)

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
            path: '/mcp-servers',
            element: (
              <EntityLayout
                titleKey="mcpServers"
                list={<McpServersListColumn width={ENTITY_LIST_WIDTH} />}
              />
            ),
            children: [
              { index: true, element: <McpServersIndexPage /> },
              { path: 'new', element: <McpServerCreatePage /> },
              { path: ':serverId', element: <McpServerDetailPage /> },
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
