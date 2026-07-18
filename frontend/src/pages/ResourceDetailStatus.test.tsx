import { cleanup, render, screen } from '@testing-library/react'
import type { ComponentType } from 'react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { afterEach, describe, expect, it, vi } from 'vitest'

import i18n from '@/i18n'
import { AgentDetailPage } from '@/pages/agents/AgentDetailPage'
import { ProviderDetailPage } from '@/pages/providers/ProviderDetailPage'
import { SkillDetailPage } from '@/pages/skills/SkillDetailPage'
import { WorkspaceDetailPage } from '@/pages/workspace/WorkspaceDetailPage'

const mocks = vi.hoisted(() => ({
  agent: {
    id: 'agent-1', name: 'Agent one', description: null, system_prompt: 'Keep this prompt.',
    llm_config: null, tool_config: null, runtime_kind: 'llm_chat' as const, acp_runtime: null,
    workspace_id: 'workspace-1', llm_provider_id: null, skill_ids: [], visibility: 'private',
    status: 'active', created_at: '2026-07-18T00:00:00Z',
  },
  provider: {
    id: 'provider-1', name: 'Provider one', kind: 'openai-compatible' as const, base_url: null,
    api_key_masked: '***', default_model: 'model-id', context_window_tokens: null,
    context_output_reserve_ratio: null, description: null, reasoning_passback: false,
    status: 'active', created_at: '2026-07-18T00:00:00Z',
  },
  skill: {
    id: 'skill-1', name: 'Skill one', description: null, body_markdown: '# Body', metadata: null,
    source: 'import', files: null, storage_path: null, status: 'active',
    created_at: '2026-07-18T00:00:00Z',
  },
  workspace: {
    id: 'workspace-1', name: 'Workspace one', backend_type: 'local' as const,
    local_path: 'D:/workspace', sandbox_ref: null, config: null, status: 'active',
    created_at: '2026-07-18T00:00:00Z', updated_at: '2026-07-18T00:00:00Z',
  },
}))

vi.mock('@/hooks/useAgents', () => ({
  useAgent: () => ({ data: mocks.agent, isLoading: false, error: null }),
  useAgents: () => ({ data: [], isLoading: false }),
}))
vi.mock('@/hooks/useDeleteAgent', () => ({
  useDeleteAgent: () => ({ mutateAsync: vi.fn(), isPending: false }),
}))
vi.mock('@/hooks/useProviders', () => ({
  useProviders: () => ({ data: [], isLoading: false }),
  useProvider: () => ({ data: mocks.provider, isLoading: false, error: null }),
  useDeleteProvider: () => ({ mutateAsync: vi.fn(), isPending: false }),
}))
vi.mock('@/hooks/useSkills', () => ({
  useSkills: () => ({ data: [], isLoading: false }),
  useSkill: () => ({ data: mocks.skill, isLoading: false, error: null }),
  useDeleteSkill: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useSkillResources: () => ({ data: [], error: null }),
  useSkillResource: () => ({ data: undefined, isLoading: false, error: null }),
  useUpdateSkillResource: () => ({ mutateAsync: vi.fn(), isPending: false }),
}))
vi.mock('@/hooks/useWorkspaces', () => ({
  useWorkspaces: () => ({ data: [mocks.workspace], isLoading: false, error: null }),
  useUpdateWorkspace: () => ({ mutate: vi.fn(), isPending: false }),
  useDeleteWorkspace: () => ({ mutateAsync: vi.fn(), isPending: false }),
}))
vi.mock('@/hooks/useGroups', () => ({
  useGroups: () => ({ data: [], isLoading: false }),
}))

function renderPage(Page: ComponentType, path: string) {
  const section = path.split('/')[1]
  const param = section === 'agents' ? 'agentId' : section === 'providers' ? 'providerId' : section === 'skills' ? 'skillId' : 'workspaceId'
  render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route path={`/${section}/:${param}`} element={<Page />} />
      </Routes>
    </MemoryRouter>,
  )
}

describe('resource detail status labels', () => {
  afterEach(async () => {
    cleanup()
    await i18n.changeLanguage('en-US')
  })

  it.each([
    [AgentDetailPage, '/agents/agent-1'],
    [ProviderDetailPage, '/providers/provider-1'],
    [SkillDetailPage, '/skills/skill-1'],
    [WorkspaceDetailPage, '/workspaces/workspace-1'],
  ] as const)('renders active semantically in Chinese for %p', async (Page, path) => {
    await i18n.changeLanguage('zh-CN')
    renderPage(Page, path)

    expect(screen.getByText('启用')).toBeInTheDocument()
    expect(screen.queryByText('active')).not.toBeInTheDocument()
  })
})
