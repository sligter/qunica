import { lazy } from 'react'
import { Navigate, type RouteObject } from 'react-router-dom'

import { EntityLayout } from '@/components/layout/EntityLayout'
import { LegacyDetailRedirect } from '@/components/layout/LegacyDetailRedirect'
import { OverlayRedirect } from '@/components/layout/overlayRouting'
import { SettingsLayout } from '@/components/layout/SettingsLayout'
import { GroupChatPage } from '@/pages/group/GroupChatPage'
import { DirectChatPage } from '@/pages/chat/DirectChatPage'
import { ChatHomePage } from '@/pages/home/ChatHomePage'

// Auth is above this tree; the layout shell and the three chat surfaces stay
// eager because they are the landing surfaces, and splitting them would only
// add a blank frame on boot. Everything below is reachable solely by
// navigation, so its code — including the react-hook-form/zod machinery on the
// create forms — loads when the route is entered.
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
const MediaSettingsPage = lazy(() =>
  import('@/pages/settings/MediaSettingsPage').then((m) => ({ default: m.MediaSettingsPage })),
)
const AppActionsPage = lazy(() =>
  import('@/pages/settings/AppActionsPage').then((m) => ({ default: m.AppActionsPage })),
)
const SystemLogsPage = lazy(() =>
  import('@/pages/settings/SystemLogsPage').then((m) => ({ default: m.SystemLogsPage })),
)
const TokenUsagePage = lazy(() =>
  import('@/pages/usage/TokenUsagePage').then((m) => ({ default: m.TokenUsagePage })),
)
const GroupManagePage = lazy(() =>
  import('@/pages/group/GroupManagePage').then((m) => ({ default: m.GroupManagePage })),
)

/**
 * Every route reachable under `AppLayout`.
 *
 * Shared between the router (which nests these under `<AppLayout/>`) and
 * `AppLayout` itself (which re-matches them at the background and overlay
 * locations to implement the settings overlay). Keeping one array in one place
 * means the stage and the panel can never drift to different trees.
 */
export const appChildren: RouteObject[] = [
  { path: '/', element: <ChatHomePage /> },
  { path: '/groups', element: <Navigate to="/" replace /> },
  { path: '/groups/:groupId/manage', element: <GroupManagePage /> },
  { path: '/groups/:groupId', element: <GroupChatPage /> },
  { path: '/chats/:chatId', element: <DirectChatPage /> },
  // Token usage is a report rather than a collection, so it takes the library
  // shell without a list column — the rail is what it needs from here.
  {
    path: '/usage',
    element: <EntityLayout titleKey="usage" />,
    children: [{ index: true, element: <TokenUsagePage /> }],
  },
  // Top-level entity areas: the sidebar Library and the resource rail are their
  // entries.
  {
    path: '/agents',
    element: <EntityLayout titleKey="agents" />,
    children: [
      { index: true, element: <AgentsIndexPage /> },
      { path: 'new', element: <AgentCreatePage /> },
      { path: ':agentId', element: <AgentDetailPage /> },
    ],
  },
  {
    path: '/providers',
    element: <EntityLayout titleKey="providers" />,
    children: [
      { index: true, element: <ProvidersIndexPage /> },
      { path: 'new', element: <ProviderCreatePage /> },
      { path: ':providerId', element: <ProviderDetailPage /> },
    ],
  },
  {
    path: '/mcp-servers',
    element: <EntityLayout titleKey="mcpServers" />,
    children: [
      { index: true, element: <McpServersIndexPage /> },
      { path: 'new', element: <McpServerCreatePage /> },
      { path: ':serverId', element: <McpServerDetailPage /> },
    ],
  },
  {
    path: '/skills',
    element: <EntityLayout titleKey="skills" />,
    children: [
      { index: true, element: <SkillsIndexPage /> },
      { path: 'new', element: <SkillCreatePage /> },
      { path: ':skillId', element: <SkillDetailPage /> },
    ],
  },
  {
    path: '/workspaces',
    element: <EntityLayout titleKey="workspaces" />,
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
    children: [
      { index: true, element: <OverlayRedirect to="/settings/system" /> },
      { path: 'system', element: <SystemSettingsPage /> },
      { path: 'media', element: <MediaSettingsPage /> },
      { path: 'logs', element: <SystemLogsPage /> },
      { path: 'assistant-actions', element: <AppActionsPage /> },
    ],
  },
  // Legacy deep links. Every redirect passes its location state through so a
  // link followed from a conversation keeps the overlay it was opened in.
  { path: '/settings/general', element: <OverlayRedirect to="/settings/system" /> },
  { path: '/settings/agents', element: <OverlayRedirect to="/agents" /> },
  { path: '/settings/agents/new', element: <OverlayRedirect to="/agents/new" /> },
  { path: '/settings/agents/:id', element: <LegacyDetailRedirect base="/agents" /> },
  { path: '/settings/providers', element: <OverlayRedirect to="/providers" /> },
  { path: '/settings/providers/new', element: <OverlayRedirect to="/providers/new" /> },
  { path: '/settings/providers/:id', element: <LegacyDetailRedirect base="/providers" /> },
  { path: '/settings/skills', element: <OverlayRedirect to="/skills" /> },
  { path: '/settings/skills/new', element: <OverlayRedirect to="/skills/new" /> },
  { path: '/settings/skills/:id', element: <LegacyDetailRedirect base="/skills" /> },
  { path: '/settings/workspaces', element: <OverlayRedirect to="/workspaces" /> },
  { path: '/settings/workspaces/new', element: <OverlayRedirect to="/workspaces/new" /> },
  { path: '/settings/workspaces/:id', element: <LegacyDetailRedirect base="/workspaces" /> },
]