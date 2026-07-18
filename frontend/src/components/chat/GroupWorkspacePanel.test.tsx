import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { GroupWorkspacePanel } from '@/components/chat/GroupWorkspacePanel'
import i18n from '@/i18n'

function renderPanel() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={queryClient}>
      <GroupWorkspacePanel groupId={undefined} />
    </QueryClientProvider>,
  )
}

describe('GroupWorkspacePanel i18n', () => {
  beforeEach(async () => {
    sessionStorage.clear()
    await i18n.changeLanguage('en-US')
  })
  afterEach(cleanup)

  it('renders the workspace tabs in English', () => {
    renderPanel()
    expect(screen.getByRole('tab', { name: 'Files' })).toBeVisible()
    expect(screen.getByRole('tab', { name: 'Git' })).toBeVisible()
  })

  it('renders the workspace tabs in Chinese', async () => {
    await i18n.changeLanguage('zh-CN')
    renderPanel()
    expect(screen.getByRole('tab', { name: '文件' })).toBeVisible()
    expect(screen.getByRole('tab', { name: 'Git' })).toBeVisible()
  })
})
