import { cleanup, render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, describe, expect, it } from 'vitest'

import i18n from '@/i18n'
import { AgentsIndexPage } from '@/pages/agents/AgentsIndexPage'
import { ProvidersIndexPage } from '@/pages/providers/ProvidersIndexPage'
import { McpServersIndexPage } from '@/pages/mcp/McpServersIndexPage'
import { SkillsIndexPage } from '@/pages/skills/SkillsIndexPage'
import { WorkspacesIndexPage } from '@/pages/workspace/WorkspacesIndexPage'

const cases = [
  [AgentsIndexPage, 'Agents', 'Agent'],
  [ProvidersIndexPage, 'LLM providers', 'LLM 服务商'],
  [McpServersIndexPage, 'MCP servers', 'MCP 服务'],
  [SkillsIndexPage, 'Skills', '技能'],
  [WorkspacesIndexPage, 'Workspaces', '工作区'],
] as const

function renderWithClient(element: React.ReactElement) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        {element}
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

describe('resource index pages', () => {
  afterEach(async () => {
    cleanup()
    await i18n.changeLanguage('en-US')
  })

  it.each(cases)('renders %p empty-state heading in English and Chinese', async (Page, english, chinese) => {
    const view = renderWithClient(<Page />)
    expect(screen.getByRole('heading', { name: english })).toBeInTheDocument()

    await i18n.changeLanguage('zh-CN')
    view.rerender(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <MemoryRouter>
          <Page />
        </MemoryRouter>
      </QueryClientProvider>,
    )
    expect(screen.getByRole('heading', { name: chinese })).toBeInTheDocument()
  })
})
