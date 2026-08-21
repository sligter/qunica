import { cleanup, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, describe, expect, it } from 'vitest'

import i18n from '@/i18n'
import { AgentsIndexPage } from '@/pages/agents/AgentsIndexPage'
import { ProvidersIndexPage } from '@/pages/providers/ProvidersIndexPage'
import { SkillsIndexPage } from '@/pages/skills/SkillsIndexPage'
import { WorkspacesIndexPage } from '@/pages/workspace/WorkspacesIndexPage'

const cases = [
  [AgentsIndexPage, 'Select an agent', '选择 Agent'],
  [ProvidersIndexPage, 'Select a provider', '选择服务商'],
  [SkillsIndexPage, 'Select a skill', '选择技能'],
  [WorkspacesIndexPage, 'Select a workspace', '选择工作区'],
] as const

describe('resource index pages', () => {
  afterEach(async () => {
    cleanup()
    await i18n.changeLanguage('en-US')
  })

  it.each(cases)('renders %p empty-state heading in English and Chinese', async (Page, english, chinese) => {
    const view = render(
      <MemoryRouter>
        <Page />
      </MemoryRouter>,
    )
    expect(screen.getByRole('heading', { name: english })).toBeInTheDocument()

    await i18n.changeLanguage('zh-CN')
    view.rerender(
      <MemoryRouter>
        <Page />
      </MemoryRouter>,
    )
    expect(screen.getByRole('heading', { name: chinese })).toBeInTheDocument()
  })
})
