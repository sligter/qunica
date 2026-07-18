import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { GroupNotesPanel } from '@/components/chat/GroupNotesPanel'
import i18n from '@/i18n'
import type { GroupNoteRead } from '@/types/api'

const rawNote: GroupNoteRead = {
  id: 'note-raw-id',
  group_id: 'group-1',
  title: 'TITLE_RAW_原文',
  content: '',
  created_at: '2026-07-18T00:00:00Z',
  updated_at: '2026-07-18T00:00:00Z',
}

function renderPanel() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  queryClient.setQueryData(['groups', 'group-1', 'notes'], [rawNote])
  return render(
    <QueryClientProvider client={queryClient}>
      <GroupNotesPanel groupId="group-1" />
    </QueryClientProvider>,
  )
}

describe('GroupNotesPanel i18n', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US')
  })
  afterEach(cleanup)

  it('renders English note controls and preserves the stored note title', () => {
    renderPanel()

    expect(screen.getByRole('heading', { name: 'Group notes' })).toBeVisible()
    expect(screen.getByText('TITLE_RAW_原文')).toBeVisible()
    expect(screen.getByText('Empty note')).toBeVisible()
    expect(screen.getByRole('button', { name: 'Delete note TITLE_RAW_原文' })).toBeVisible()
  })

  it('renders the create-note form in Chinese', async () => {
    await i18n.changeLanguage('zh-CN')
    renderPanel()

    fireEvent.click(screen.getByRole('button', { name: '新建笔记' }))
    expect(screen.getByLabelText('标题')).toHaveAttribute('placeholder', '笔记标题')
    expect(screen.getByLabelText('内容')).toHaveAttribute('placeholder', '写下笔记…')
    expect(screen.getByRole('button', { name: '取消' })).toBeVisible()
    expect(screen.getByRole('button', { name: '保存' })).toBeVisible()
  })
})
