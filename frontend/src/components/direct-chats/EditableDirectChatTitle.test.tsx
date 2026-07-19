import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { I18nextProvider } from 'react-i18next'
import { describe, expect, it, vi } from 'vitest'

import { EditableDirectChatTitle } from './EditableDirectChatTitle'
import i18n from '@/i18n'

const mutateAsync = vi.fn()
vi.mock('@/hooks/useDirectChats', () => ({
  useRenameDirectChat: () => ({ mutateAsync }),
}))

describe('EditableDirectChatTitle', () => {
  it('saves a trimmed title with Enter', async () => {
    const user = userEvent.setup()
    mutateAsync.mockResolvedValueOnce(undefined)
    render(<I18nextProvider i18n={i18n}><EditableDirectChatTitle chatId="chat-1" title="Original" /></I18nextProvider>)
    await user.click(screen.getByRole('button', { name: 'Rename conversation' }))
    const input = screen.getByRole('textbox', { name: 'Rename conversation' })
    await user.clear(input)
    await user.type(input, '  Pinned title  {Enter}')
    expect(mutateAsync).toHaveBeenCalledWith({ title: 'Pinned title' })
  })
})
