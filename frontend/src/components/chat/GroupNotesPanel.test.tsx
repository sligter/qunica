import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

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

function renderPanel(content?: string) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  queryClient.setQueryData(['groups', 'group-1', 'notes'], [rawNote])
  if (content !== undefined) queryClient.setQueryData(['groups', 'group-1', 'notes', rawNote.id], { ...rawNote, content })
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
  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  it('renders English note controls and preserves the stored note title', () => {
    renderPanel()

    expect(screen.getByRole('heading', { name: 'Group notes' })).toBeVisible()
    expect(screen.getByText('TITLE_RAW_原文')).toBeVisible()
    expect(screen.getByText('Open to load')).toBeVisible()
    expect(screen.getByRole('button', { name: 'Delete note TITLE_RAW_原文' })).toBeVisible()
  })

  it('renders the create-note form in Chinese', async () => {
    await i18n.changeLanguage('zh-CN')
    renderPanel()

    fireEvent.click(screen.getByRole('button', { name: '新建笔记' }))
    expect(screen.getByLabelText('标题')).toHaveAttribute('placeholder', '笔记标题')
    expect(screen.getByLabelText('内容')).toHaveAttribute('placeholder', '写下笔记…')
    expect((screen.getByLabelText('内容') as HTMLTextAreaElement).value).toContain('Status: proposed')
    expect(screen.getByLabelText('状态')).toHaveTextContent('提议中')
    expect(screen.getByLabelText('类别')).toHaveTextContent('决策')
    fireEvent.change(screen.getByLabelText('标题'), { target: { value: '选用 SQLite' } })
    expect((screen.getByLabelText('内容') as HTMLTextAreaElement).value).toContain('# 选用 SQLite\n')
    expect(screen.getByRole('button', { name: '取消' })).toBeVisible()
    expect(screen.getByRole('button', { name: '保存' })).toBeVisible()
  })

  it('keeps legacy content intact when its title changes', () => {
    renderPanel('Legacy notes\nDo not rewrite this.')
    fireEvent.click(screen.getByText(rawNote.title))
    expect(screen.queryByLabelText('Status')).not.toBeInTheDocument()
    fireEvent.change(screen.getByLabelText('Title'), { target: { value: 'Renamed' } })
    expect(screen.getByLabelText('Content')).toHaveValue('Legacy notes\nDo not rewrite this.')
  })

  it('saves the chosen lifecycle and category in the Markdown sent to the API', async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify(rawNote), { status: 201 }))
    vi.stubGlobal('fetch', fetchMock)
    renderPanel()
    fireEvent.click(screen.getByRole('button', { name: 'New note' }))
    fireEvent.change(screen.getByLabelText('Title'), { target: { value: 'Decision' } })
    fireEvent.keyDown(screen.getByLabelText('Status'), { key: 'ArrowDown' })
    fireEvent.click(await screen.findByRole('option', { name: 'Implemented' }))
    fireEvent.keyDown(screen.getByLabelText('Category'), { key: 'ArrowDown' })
    fireEvent.click(await screen.findByRole('option', { name: 'Convention' }))
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledOnce())
    const request = fetchMock.mock.calls[0][1] as RequestInit
    expect(request.method).toBe('POST')
    const body = JSON.parse(request.body as string)
    expect(body.title).toBe('Decision')
    expect(body.content).toContain('# Decision\nStatus: implemented\n')
    expect(body.content).toContain('Category: 约定\n')
    await screen.findByRole('button', { name: 'New note' })
  })

  it('requires a rejection reason and preserves the stored initial date', () => {
    renderPanel('# Rejected\nStatus: rejected\nSince: 2025-01-02\nCategory: 决策\n\n## Problem\nOriginal problem')
    fireEvent.click(screen.getByText(rawNote.title))
    expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled()
    fireEvent.change(screen.getByLabelText('Rejection reason (required)'), { target: { value: 'Too costly' } })
    expect(screen.getByRole('button', { name: 'Save' })).toBeEnabled()
    expect((screen.getByLabelText('Content') as HTMLTextAreaElement).value).toContain('Status: rejected — Too costly\nSince: 2025-01-02')
  })

  it.each([
    ['create', 'en-US', 'The note is saved, but the shared notes index could not be updated. Do not save it again to retry.', 'note_index_pending'],
    ['update', 'en-US', 'The note is saved, but the shared notes index could not be updated. Do not save it again to retry.', 'note_index_pending'],
    ['create', 'zh-CN', '笔记已保存，但共享笔记索引未能更新。请勿通过再次保存来重试。', 'note_index_pending'],
    ['create', 'en-US', 'The note is saved, but its workspace file could not be created. Do not create it again to retry.', 'note_content_pending'],
    ['create', 'zh-CN', '笔记已保存，但工作区中的正文文件未能创建。请勿再次创建来重试。', 'note_content_pending'],
  ] as const)('acknowledges a saved %s in %s without offering the save again', async (action, locale, message, code) => {
    await i18n.changeLanguage(locale)
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({
      ...rawNote,
      content: 'Saved body',
      warning: { code, reason: 'A fixture filesystem failure' },
    }), { status: action === 'create' ? 201 : 200 }))
    vi.stubGlobal('fetch', fetchMock)
    renderPanel('Saved body')
    const newLabel = locale === 'zh-CN' ? '新建笔记' : 'New note'
    const saveLabel = locale === 'zh-CN' ? '保存' : 'Save'
    fireEvent.click(action === 'create'
      ? screen.getByRole('button', { name: newLabel })
      : screen.getByText(rawNote.title))
    if (action === 'create') fireEvent.change(screen.getByLabelText(locale === 'zh-CN' ? '标题' : 'Title'), { target: { value: 'Saved' } })
    fireEvent.click(screen.getByRole('button', { name: saveLabel }))
    expect(await screen.findByRole('status')).toHaveTextContent(message)
    expect(screen.queryByRole('button', { name: saveLabel })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: newLabel })).toBeVisible()
    expect(fetchMock).toHaveBeenCalledOnce()
    expect((fetchMock.mock.calls[0][1] as RequestInit).method).toBe(action === 'create' ? 'POST' : 'PATCH')
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: newLabel }))
    expect(screen.queryByRole('status')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: saveLabel })).toBeVisible()
    expect(fetchMock).toHaveBeenCalledOnce()
  })
})
